package dev.tgorka.keeper

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.speech.RecognitionListener
import android.speech.RecognitionSupport
import android.speech.RecognitionSupportCallback
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import android.speech.tts.Voice
import androidx.annotation.RequiresApi
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.Locale
import kotlin.math.sqrt

@InvokeArg
class VoiceAttachArgs {
  lateinit var events: Channel
}

@InvokeArg
class VoiceListenArgs {
  var session: Long = 0
  lateinit var locale: String
}

@InvokeArg
class VoiceSessionArgs {
  var session: Long = 0
}

@InvokeArg
class VoiceSpeakArgs {
  lateinit var text: String
  lateinit var language: String
  var flush: Boolean = true
}

/**
 * The Kotlin half of keeper's Android voice port (Epic 98, story 98.4; AD-414,
 * D-36; `voice_android.rs`). It decides nothing: whether keeper may listen, in
 * which language, what a recogniser error means and what is said are
 * `keeper_core::voice`'s, and this file carries them out and reports back.
 *
 * - **Facts.** `facts` reads what Rust decides over: the API level, the
 *   microphone grant, whether an on-device recogniser exists, the languages it
 *   has installed and could download, the system locale, whether the device has
 *   an echo canceller, and the languages of the installed voices that need no
 *   network.
 * - **Capture.** `listen` records the microphone itself — `AudioRecord` on
 *   `VOICE_COMMUNICATION` with the device's `AcousticEchoCanceler` on it where
 *   there is one, so keeper's own voice is kept out — and feeds that audio to the
 *   on-device recogniser through a pipe (`EXTRA_AUDIO_SOURCE`) in a segmented
 *   session. Every recogniser is made with
 *   `SpeechRecognizer.createOnDeviceSpeechRecognizer` after
 *   `SpeechRecognizer.isOnDeviceRecognitionAvailable`: no other kind exists in
 *   keeper, and `keeper-core/tests/voice_on_device.rs` fails the build if one
 *   appears. The capture, once up, stays up until `stop`: a `listen` while it
 *   records and an `again` only start a fresh session on it, so the words said
 *   while a session rolls are not lost to a recorder being rebuilt. `finish`
 *   closes the session's audio so its last words come; `stop` ends both.
 * - **Speech.** `TextToSpeech` in the installed voice for the language Rust
 *   names, one utterance at a time from keeper's own queue, so a pause can stop
 *   the engine (which has no pause of its own) and read on from the last word it
 *   reached.
 * - **The front.** Android gives an app with no microphone foreground service
 *   silence once it leaves the front, and keeper runs none: a pause is reported
 *   as `background`, and the return to the front after one as `front`.
 *
 * Every recogniser event carries the session Rust named
 * (`keeper_core::voice::android::Sessions`), so Rust drops what a session it
 * has rolled or stopped still says.
 */
@TauriPlugin(
  permissions = [
    Permission(strings = [Manifest.permission.RECORD_AUDIO], alias = "microphone"),
  ],
)
class VoicePlugin(private val activity: Activity) : Plugin(activity) {
  private val main = Handler(Looper.getMainLooper())
  private var events: Channel? = null

  // The capture.
  @Volatile private var session: Long = 0
  private var locale: String = ""
  private var recorder: AudioRecord? = null
  private var canceller: AcousticEchoCanceler? = null
  private var reader: Thread? = null
  @Volatile private var recording = false
  private val sinkLock = Any()
  private var sink: ParcelFileDescriptor.AutoCloseOutputStream? = null
  private var source: ParcelFileDescriptor? = null
  private var recognizer: SpeechRecognizer? = null

  // Speech.
  private var tts: TextToSpeech? = null
  private var ttsReady = false
  private val afterTts = ArrayList<() -> Unit>()
  private val lines = ArrayDeque<Line>()
  private var paused = false
  private var reachedInHead = 0
  private var utterances = 0L

  // The front.
  private var wasPaused = false

  private class Line(val id: String, var text: String, val voice: Voice)

  @Command
  fun attach(invoke: Invoke) {
    events = invoke.parseArgs(VoiceAttachArgs::class.java).events
    invoke.resolve()
  }

  @Command
  fun facts(invoke: Invoke) {
    withTts {
      onDeviceLanguages { installed, downloadable ->
        val answer = JSObject()
        answer.put("sdkInt", Build.VERSION.SDK_INT)
        answer.put("microphone", (getPermissionState("microphone") ?: "prompt").toString())
        answer.put("recogniser", recogniserAvailable())
        answer.put("installed", JSArray(installed))
        answer.put("downloadable", JSArray(downloadable))
        answer.put("system", Locale.getDefault().toLanguageTag())
        answer.put("echoCanceller", AcousticEchoCanceler.isAvailable())
        answer.put("voices", JSArray(offlineVoices().map { it.locale.toLanguageTag() }.distinct()))
        invoke.resolve(answer)
      }
    }
  }

  @Command
  fun listen(invoke: Invoke) {
    val args = invoke.parseArgs(VoiceListenArgs::class.java)
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || !recogniserAvailable()) {
      stopCapture()
      invoke.reject("no on-device recogniser on this device")
      return
    }
    if (getPermissionState("microphone")?.toString() != "granted") {
      stopCapture()
      invoke.reject("the microphone is not allowed")
      return
    }
    session = args.session
    locale = args.locale
    if (!recording && !startCapture()) {
      send(recogniserEvent("error").also { it.put("code", SpeechRecognizer.ERROR_AUDIO) })
      invoke.resolve()
      return
    }
    startSession()
    invoke.resolve()
  }

  @Command
  fun again(invoke: Invoke) {
    val args = invoke.parseArgs(VoiceListenArgs::class.java)
    if (recording && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      session = args.session
      locale = args.locale
      startSession()
    }
    invoke.resolve()
  }

  @Command
  fun finish(invoke: Invoke) {
    val args = invoke.parseArgs(VoiceSessionArgs::class.java)
    if (args.session == session) endAudio()
    invoke.resolve()
  }

  @Command
  fun stop(invoke: Invoke) {
    stopCapture()
    invoke.resolve()
  }

  @Command
  fun speak(invoke: Invoke) {
    val args = invoke.parseArgs(VoiceSpeakArgs::class.java)
    withTts {
      if (args.flush) {
        lines.clear()
        paused = false
        tts?.stop()
      }
      val voice = offlineVoices()
        .filter { it.locale.toLanguageTag().equals(args.language, ignoreCase = true) }
        .maxByOrNull { it.quality }
      if (voice == null) {
        // Rust chose the language from the voices `facts` listed; one gone since
        // is read as nothing, so the turn is not left waiting for it.
        send(event("spoken"))
      } else {
        utterances += 1
        lines.addLast(Line("keeper-$utterances", args.text, voice))
        if (lines.size == 1 && !paused) speakHead()
      }
      invoke.resolve()
    }
  }

  @Command
  fun stopSpeaking(invoke: Invoke) {
    lines.clear()
    paused = false
    tts?.stop()
    invoke.resolve()
  }

  @Command
  fun pauseSpeaking(invoke: Invoke) {
    paused = true
    tts?.stop()
    invoke.resolve()
  }

  @Command
  fun continueSpeaking(invoke: Invoke) {
    paused = false
    lines.firstOrNull()?.let { head ->
      head.text = head.text.substring(reachedInHead.coerceIn(0, head.text.length))
    }
    speakHead()
    invoke.resolve()
  }

  override fun onPause() {
    wasPaused = true
    send(event("background"))
  }

  override fun onResume() {
    // Only a return: at launch Rust reads the device itself (`set_up`), and
    // the switch the person left on is armed once those facts are in.
    if (wasPaused) {
      wasPaused = false
      send(event("front"))
    }
  }

  override fun onDestroy() {
    stopCapture()
    tts?.shutdown()
    tts = null
  }

  // -- Facts -------------------------------------------------------------------

  private fun recogniserAvailable(): Boolean =
    Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
      SpeechRecognizer.isOnDeviceRecognitionAvailable(activity)

  /** The on-device recogniser's installed and downloadable languages. */
  private fun onDeviceLanguages(answer: (List<String>, List<String>) -> Unit) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || !recogniserAvailable()) {
      answer(emptyList(), emptyList())
      return
    }
    var answered = false
    val once = { installed: List<String>, downloadable: List<String> ->
      if (!answered) {
        answered = true
        answer(installed, downloadable)
      }
    }
    val checker = SpeechRecognizer.createOnDeviceSpeechRecognizer(activity)
    checker.checkRecognitionSupport(
      Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).putExtra(
        RecognizerIntent.EXTRA_LANGUAGE_MODEL,
        RecognizerIntent.LANGUAGE_MODEL_FREE_FORM,
      ),
      ContextCompat.getMainExecutor(activity),
      object : RecognitionSupportCallback {
        override fun onSupportResult(support: RecognitionSupport) {
          val installed = support.installedOnDeviceLanguages
          val downloadable = (support.supportedOnDeviceLanguages + support.pendingOnDeviceLanguages)
            .filter { it !in installed }
            .distinct()
          checker.destroy()
          once(installed, downloadable)
        }

        override fun onError(error: Int) {
          checker.destroy()
          once(emptyList(), emptyList())
        }
      },
    )
    main.postDelayed({ once(emptyList(), emptyList()) }, SUPPORT_WAIT_MS)
  }

  // -- Capture -----------------------------------------------------------------

  @SuppressLint("MissingPermission") // checked in `listen` before this runs
  private fun startCapture(): Boolean {
    val minimum = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
    val record = AudioRecord(
      MediaRecorder.AudioSource.VOICE_COMMUNICATION,
      RATE,
      AudioFormat.CHANNEL_IN_MONO,
      AudioFormat.ENCODING_PCM_16BIT,
      maxOf(minimum, FRAME * 2 * 4),
    )
    if (record.state != AudioRecord.STATE_INITIALIZED) {
      record.release()
      return false
    }
    if (AcousticEchoCanceler.isAvailable()) {
      canceller = AcousticEchoCanceler.create(record.audioSessionId)?.also { it.enabled = true }
    }
    record.startRecording()
    recorder = record
    recording = true
    reader = Thread({ pump(record) }, "keeper-voice-capture").also { it.start() }
    return true
  }

  /**
   * Read the microphone and hand it to the current session's pipe, with its
   * level tagged by that session: the capture outlives every session on it.
   */
  private fun pump(record: AudioRecord) {
    val samples = ShortArray(FRAME)
    val bytes = ByteBuffer.allocate(FRAME * 2).order(ByteOrder.LITTLE_ENDIAN)
    while (recording) {
      val read = record.read(samples, 0, FRAME)
      if (read <= 0) continue
      var squares = 0.0
      bytes.clear()
      for (i in 0 until read) {
        val sample = samples[i] / 32768.0
        squares += sample * sample
        bytes.putShort(samples[i])
      }
      val level = JSObject()
      level.put("kind", "level")
      level.put("session", session)
      level.put("rms", sqrt(squares / read))
      send(level)
      val out = synchronized(sinkLock) { sink } ?: continue
      try {
        out.write(bytes.array(), 0, read * 2)
      } catch (e: IOException) {
        // The session's end closed the pipe; the next session brings a new one.
        synchronized(sinkLock) { if (sink === out) sink = null }
      }
    }
  }

  /** A fresh recogniser session on the capture, with a pipe of its own. */
  @RequiresApi(Build.VERSION_CODES.TIRAMISU)
  private fun startSession() {
    endSession()
    val pipe = ParcelFileDescriptor.createPipe()
    synchronized(sinkLock) { sink = ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]) }
    source = pipe[0]
    val next = SpeechRecognizer.createOnDeviceSpeechRecognizer(activity)
    next.setRecognitionListener(Listener(session))
    val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
      .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
      .putExtra(RecognizerIntent.EXTRA_LANGUAGE, locale)
      .putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, true)
      .putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
      .putExtra(RecognizerIntent.EXTRA_AUDIO_SOURCE, pipe[0])
      .putExtra(RecognizerIntent.EXTRA_AUDIO_SOURCE_CHANNEL_COUNT, 1)
      .putExtra(RecognizerIntent.EXTRA_AUDIO_SOURCE_ENCODING, AudioFormat.ENCODING_PCM_16BIT)
      .putExtra(RecognizerIntent.EXTRA_AUDIO_SOURCE_SAMPLING_RATE, RATE)
      .putExtra(RecognizerIntent.EXTRA_SEGMENTED_SESSION, RecognizerIntent.EXTRA_AUDIO_SOURCE)
    next.startListening(intent)
    recognizer = next
  }

  /** Close the session's audio: the recogniser reads to the end and says its last words. */
  private fun endAudio() {
    val out = synchronized(sinkLock) { sink.also { sink = null } }
    try {
      out?.close()
    } catch (e: IOException) {
      // Already closed by the recogniser's end.
    }
  }

  private fun endSession() {
    endAudio()
    recognizer?.destroy()
    recognizer = null
    try {
      source?.close()
    } catch (e: IOException) {
      // Already closed.
    }
    source = null
  }

  private fun stopCapture() {
    recording = false
    endSession()
    reader?.join(READER_JOIN_MS)
    reader = null
    recorder?.let {
      try {
        it.stop()
      } catch (e: IllegalStateException) {
        // Never started.
      }
      it.release()
    }
    recorder = null
    canceller?.release()
    canceller = null
  }

  private inner class Listener(private val tag: Long) : RecognitionListener {
    private fun words(results: Bundle?): String =
      results?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull() ?: ""

    private fun report(kind: String, text: String? = null) {
      val event = JSObject()
      event.put("kind", kind)
      event.put("session", tag)
      if (text != null) event.put("text", text)
      send(event)
    }

    override fun onPartialResults(partialResults: Bundle?) = report("partial", words(partialResults))

    override fun onSegmentResults(segmentResults: Bundle) = report("segment", words(segmentResults))

    override fun onEndOfSegmentedSession() = report("ended")

    override fun onResults(results: Bundle?) {
      report("segment", words(results))
      report("ended")
    }

    override fun onError(error: Int) {
      val event = JSObject()
      event.put("kind", "error")
      event.put("session", tag)
      event.put("code", error)
      send(event)
    }

    override fun onReadyForSpeech(params: Bundle?) {}

    override fun onBeginningOfSpeech() {}

    override fun onRmsChanged(rmsdB: Float) {}

    override fun onBufferReceived(buffer: ByteArray?) {}

    override fun onEndOfSpeech() {}

    override fun onEvent(eventType: Int, params: Bundle?) {}
  }

  // -- Speech ------------------------------------------------------------------

  /** Run `then` once the engine is up (at once when it is). */
  private fun withTts(then: () -> Unit) {
    if (ttsReady) {
      then()
      return
    }
    afterTts.add(then)
    if (tts != null) return
    tts = TextToSpeech(activity) { status ->
      main.post {
        ttsReady = true
        if (status != TextToSpeech.SUCCESS) {
          tts?.shutdown()
          tts = null
        } else {
          tts?.setAudioAttributes(
            AudioAttributes.Builder()
              .setUsage(AudioAttributes.USAGE_ASSISTANT)
              .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
              .build(),
          )
          tts?.setOnUtteranceProgressListener(Progress())
        }
        val waiting = ArrayList(afterTts)
        afterTts.clear()
        waiting.forEach { it() }
      }
    }
  }

  /** The engine's installed voices that need no network. */
  private fun offlineVoices(): List<Voice> =
    tts?.voices.orEmpty().filter {
      !it.isNetworkConnectionRequired &&
        !it.features.orEmpty().contains(TextToSpeech.Engine.KEY_FEATURE_NOT_INSTALLED)
    }

  private fun speakHead() {
    val head = lines.firstOrNull() ?: return
    if (paused) return
    reachedInHead = 0
    tts?.voice = head.voice
    tts?.speak(head.text, TextToSpeech.QUEUE_FLUSH, null, head.id)
  }

  private inner class Progress : UtteranceProgressListener() {
    override fun onStart(utteranceId: String?) {}

    override fun onRangeStart(utteranceId: String?, start: Int, end: Int, frame: Int) {
      main.post { if (lines.firstOrNull()?.id == utteranceId) reachedInHead = start }
    }

    override fun onDone(utteranceId: String?) {
      main.post { finished(utteranceId) }
    }

    @Deprecated("Deprecated in Java")
    override fun onError(utteranceId: String?) {
      main.post { finished(utteranceId) }
    }

    override fun onError(utteranceId: String?, errorCode: Int) {
      main.post { finished(utteranceId) }
    }

    // A stop or a pause: not read to its end, so nothing is reported.
    override fun onStop(utteranceId: String?, interrupted: Boolean) {}
  }

  /** The head utterance was read to its end (or could not be read): the next one. */
  private fun finished(utteranceId: String?) {
    if (paused || lines.firstOrNull()?.id != utteranceId) return
    lines.removeFirst()
    send(event("spoken"))
    speakHead()
  }

  // -- Events ------------------------------------------------------------------

  private fun event(kind: String): JSObject = JSObject().also { it.put("kind", kind) }

  private fun recogniserEvent(kind: String): JSObject =
    event(kind).also { it.put("session", session) }

  private fun send(event: JSObject) {
    events?.send(event)
  }

  companion object {
    /** The capture's rate: what the on-device recogniser is told it is fed. */
    const val RATE = 16_000

    /** 40 ms of audio: one level reading per buffer, the meter's own interval. */
    const val FRAME = 640

    const val SUPPORT_WAIT_MS = 3_000L
    const val READER_JOIN_MS = 500L
  }
}
