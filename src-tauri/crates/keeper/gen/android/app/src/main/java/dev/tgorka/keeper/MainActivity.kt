package dev.tgorka.keeper

import android.os.Build
import android.os.Bundle
import android.system.Os
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // keeper's Rust half starts inside super.onCreate (WryActivity's lifecycle
    // observer calls Rust.create, which runs `run()` and `AppState::new`) —
    // before it has an app handle or a plugin to ask. So the three facts it
    // needs that early go into the process environment first. None is a
    // secret. Read by src-tauri/crates/keeper/src/android.rs (story 98.3).
    Os.setenv("KEEPER_ANDROID_FILES_DIR", filesDir.absolutePath, true)
    Os.setenv(
      "KEEPER_ANDROID_DEVICE_CLASS",
      if (resources.configuration.smallestScreenWidthDp >= 600) "tablet" else "mobile",
      true,
    )
    Os.setenv("KEEPER_ANDROID_MODEL", Build.MODEL ?: "", true)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }
}
