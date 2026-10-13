buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        // Kotlin 2.1: androidx.browser 1.10.0 (Auth Tab, story 98.3) brings
        // kotlin-stdlib 2.1.20, whose metadata a 1.9 compiler refuses to read —
        // every Kotlin file of the app failed to compile under 1.9.25.
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.1.20")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

tasks.register("clean").configure {
    delete("build")
}

