package dev.tgorka.keeper

import android.app.Activity
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The Android Keystore's half of keeper's secret store (Epic 98, story 98.3 #2).
 *
 * keeper's secrets live in one AEAD file in the app's files directory; its
 * format is `keeper_core::secret_file`'s and is tested on Linux. This plugin
 * owns only the file's 32-byte data key: generated once at random, kept on disk
 * solely wrapped (AES-256-GCM) by a key the Android Keystore generated and never
 * lets out, and unwrapped on request for the Rust side, which asks once per
 * process (`android.rs`, `KeystoreKey`).
 *
 * A wrapped key that no longer unwraps (the Keystore key is gone or the file was
 * changed) is refused, never replaced: a new data key would silently orphan every
 * secret in the file. Starting over is clearing keeper's storage in Android's
 * settings, which removes the file, the wrapped key and the Keystore key together.
 */
@TauriPlugin
class KeystorePlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun dataKey(invoke: Invoke) {
    try {
      val answer = JSObject()
      answer.put("key", dataKeyHex())
      invoke.resolve(answer)
    } catch (e: Exception) {
      // The exception's class names what failed; its message may carry key
      // material from a provider, so it is not passed on.
      invoke.reject("the Android Keystore refused keeper's key (${e.javaClass.simpleName})")
    }
  }

  @Synchronized
  private fun dataKeyHex(): String {
    val wrapped = File(activity.noBackupFilesDir, WRAPPED_FILE)
    val key = if (wrapped.exists()) unwrap(wrapped.readBytes()) else create(wrapped)
    return key.joinToString("") { "%02x".format(it.toInt() and 0xff) }
  }

  private fun create(wrapped: File): ByteArray {
    val key = ByteArray(KEY_BYTES).also { SecureRandom().nextBytes(it) }
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
    val sealed = cipher.iv + cipher.doFinal(key)
    val tmp = File(wrapped.parentFile, "$WRAPPED_FILE.tmp")
    tmp.writeBytes(sealed)
    if (!tmp.renameTo(wrapped)) {
      tmp.delete()
      throw IllegalStateException("could not store the wrapped key")
    }
    return key
  }

  private fun unwrap(sealed: ByteArray): ByteArray {
    require(sealed.size > IV_BYTES) { "the wrapped key is truncated" }
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(
      Cipher.DECRYPT_MODE,
      existingWrappingKey(),
      GCMParameterSpec(TAG_BITS, sealed, 0, IV_BYTES),
    )
    val key = cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES)
    require(key.size == KEY_BYTES) { "the wrapped key has the wrong length" }
    return key
  }

  private fun keyStore(): KeyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }

  private fun existingWrappingKey(): SecretKey =
    keyStore().getKey(ALIAS, null) as? SecretKey
      ?: throw IllegalStateException("the Keystore key that wraps keeper's key is gone")

  private fun wrappingKey(): SecretKey {
    (keyStore().getKey(ALIAS, null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
    generator.init(
      KeyGenParameterSpec.Builder(
        ALIAS,
        KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
      )
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build(),
    )
    return generator.generateKey()
  }

  private companion object {
    const val KEYSTORE = "AndroidKeyStore"
    const val ALIAS = "keeper-secret-file-key"
    const val WRAPPED_FILE = "keeper-secret-file.key"
    const val TRANSFORMATION = "AES/GCM/NoPadding"
    const val KEY_BYTES = 32
    const val IV_BYTES = 12
    const val TAG_BITS = 128
  }
}
