# cheat-enginer overlay for Android

A floating memory scanner with scan, results, address-table and hex views, all
operated by touch and typing. The engine is the same Rust code as the desktop
TUI (`../src`), compiled to `libce_engine.so` and driven over JNI.

## Two ways to deploy it

|  | **Embedded** (`:overlay` + `:sample`) | **Standalone** (`:cheatengine` + `:game`) |
|---|---|---|
| shipped as | an AAR inside the app being inspected | its own APK with its own launcher |
| window | `TYPE_APPLICATION_PANEL` on the host Activity | `TYPE_APPLICATION_OVERLAY` from a Service |
| permissions | **none at all** | `SYSTEM_ALERT_WINDOW` + foreground service |
| the target app changes | one `debugImplementation` line | two manifest attributes |
| it reaches the target because | it is linked into it | both packages share one OS process |

Either way the scanner ends up in **the same process as the thing it scans**,
and that is the only reason it works without root: `/proc/self/mem` needs no
privilege, while reading *another* process needs `ptrace`, which stock Android's
SELinux policy denies to `untrusted_app` outright.

`WindowHost` is the only thing that differs between the two modes; everything
above it - the session, the panel, the address table - is shared.

That makes this a debugging and teaching tool: point it at your own app's
counters, timers and game state.

**One standing limitation.** Values held in ordinary Java/Kotlin fields live on
the ART heap, which the concurrent-copying collector *moves*. An address found
there goes stale after a GC, and a freeze on it silently stops working. Values
in memory ART does not move - a `ByteBuffer.allocateDirect`, anything from JNI,
anything malloc'd - have stable addresses and behave the way a desktop game
does. Both sample apps show the two side by side.

## Embedded mode: using it in your own app

```kotlin
dependencies {
    debugImplementation(project(":overlay"))
}
```

That is the whole integration. The overlay installs itself from its own
`ContentProvider` before `Application.onCreate`, so your source never references
it - which is exactly why `debugImplementation` alone keeps it out of release
builds. There is no call site to `#ifdef` away, and no no-op stub to maintain.

`:sample:verifyOverlayNotInRelease` asserts the `.so` is absent from the release
APK; copy that task if you want the same guarantee.

## Standalone mode: two APKs, one process

`:cheatengine` is the scanner with its own launcher icon. `:game` is Dungeon
Tap, an ordinary little game containing **no engine code, no native library and
no permission**. The only thing the game does on the scanner's behalf is carry
two manifest attributes:

```xml
<manifest ... android:sharedUserId="dev.marc.ce.shared" tools:ignore="Deprecated">
    <application android:process="dev.marc.ce.shared.proc" ...>
```

The engine APK declares the same two, and both are debug-signed with the same
key. That is the entire contract: Android then runs both packages in one
process, and `Session::attach_self()` sees the game's heap because in a shared
process "self" *is* the shared process. No Rust code changed for any of this.

Rules that are easy to get wrong:

- A process name starting with `:` is private to its own package and can never
  be shared. Use a global name, and it must contain a `.`.
- **A package's UID is fixed at install time.** Adding `sharedUserId` to an
  already-installed package does nothing, and reinstalling over it fails with
  `INSTALL_FAILED_UID_CHANGED` - uninstall both, then install both. The CE
  launcher checks the game's UID against its own and says so on screen, because
  the alternative symptom is "everything runs and scans just find nothing".
- Mismatched signing certificates give `INSTALL_FAILED_SHARED_USER_INCOMPATIBLE`.
- `am force-stop` on *either* package kills the shared process, taking the scan
  results and the freeze threads with it. So does reinstalling either APK.
- Launch order does not matter. Whichever app starts the process, the other
  joins it; the engine attaches when its Service starts. Nothing may depend on
  implicit initialization, though - in a shared process only the package that
  created it gets its ContentProviders installed at bind time, which is why the
  CE APK removes `OverlayInstallProvider` and drives everything from the
  Service.
- `android:sharedUserId` has been deprecated since API 29 and Play blocks new
  apps that use it. Fine for a sideloaded dev tool, and verified working on
  API 35 - but it is the one thing here that could disappear in a future
  release.

```bash
adb uninstall dev.marc.ce.app ; adb uninstall dev.marc.ce.game
gradle :cheatengine:installDebug :game:installDebug
```

Then open **Cheat Engine**, grant the overlay permission and **Start engine**.
The launcher lists every app sharing its uid, marking which of them also share
its process - those are the scannable ones - and launches whichever you tap.
Nothing is hardcoded: install a third app with the same two manifest
attributes and it shows up on its own.

To prove they really are one process:

```bash
adb shell ps -A -o PID,NAME | grep ce.shared        # exactly one row
adb shell dumpsys activity processes | grep packageList
```

or just read the pid printed on both screens - they match.

## Building

Prerequisites, all installed by:

```bash
sdkmanager "ndk;28.2.13676358"
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
```

Then:

```bash
cd android
gradle :sample:installDebug                              # embedded mode
gradle :cheatengine:installDebug :game:installDebug      # standalone mode
```

There is no Gradle wrapper checked in; on this machine the local distribution is
`C:\devtools\gradle\gradle-8.11.1\bin\gradle.bat`.

For sideloadable builds use `./pack.sh`, which builds all three debug APKs,
copies them to `dist/` with checksums, and refuses to finish if any of the
three things that break the design *silently* has happened - an engine `.so`
in the game APK, two different signing certificates, or a mismatched
`sharedUserId` / `android:process` between the two APKs. The last two are read
back out of the *built* APKs rather than the source manifests, so a manifest
merger surprise is caught too:

```bash
./pack.sh                # build + verify + package
./pack.sh --install      # also install to the attached device, in the right order
./pack.sh --test         # run cargo test first
./pack.sh --help
```

## Making it yours

Three scripts, each doing one job.

### `./keystore.sh` — the signing key

`sharedUserId` requires **the same certificate** on every participating APK, so
there is one key shared by `:cheatengine`, `:game` and `:sample`. Until you set
one up, all three use AGP's debug keystore, which is fine for sideloading and
useless for release.

```bash
./keystore.sh new --cn "Your Company Name"   # generates release.jks, prompts for a password
./keystore.sh use path/to/existing.jks       # point at a key you already have
./keystore.sh show                           # current config + certificate fingerprint
./keystore.sh clear                          # back to the debug keystore
```

It writes `signing.properties`, which is gitignored and read by all three app
modules. Changing the key changes the app's identity: installed copies must be
uninstalled first, and a released app can't change key at all without going
through Play App Signing's key rotation. **Back the keystore up** - a lost
signing key has no recovery path.

### `./rename.sh` — package names and app labels

```bash
./rename.sh --show                              # what the names are now
./rename.sh --base <your.package.base> --dry-run
./rename.sh --base <your.package.base> \
            --ce-label "<Your Scanner>" --game-label "<Your Game>"
```

`--base` drives everything: `<base>.app`, `<base>.game`, `<base>.sample`,
`<base>.overlay`, the `sharedUserId` and the `android:process` name.

The shared identity can also be set on its own, without touching the package
names - which is what you want when these two apps have to join a
`sharedUserId` that already exists:

```bash
./rename.sh --shared-user-id <existing.shared.id> --process <existing.proc.name>
```

`sharedUserId` and `android:process` live here rather than in `pack.sh`
because they are *identity*, not build options: they are written into the
installed package and cannot be changed afterwards without uninstalling. A
build-time flag would make the checked-in manifests lie - an APK built in
Android Studio would carry a different shared identity from one built by
`pack.sh`, and the symptom is not an error but "both apps install fine and
scans find nothing". So `rename.sh` sets the identity and `pack.sh` verifies
it, the same split as `keystore.sh` creating the key and `pack.sh` checking
that all three certificates match.

The reason this is a script and not a find-and-replace is item 5 below - it is
the one that compiles perfectly and then throws `UnsatisfiedLinkError` at
runtime:

1. Gradle `namespace` / `applicationId`
2. Java directory layout, `package` declarations and imports
3. `sharedUserId`, `android:process`, `<queries>`, the provider's fully-qualified name
4. Package strings hardcoded in Java (`ACTION_STOP`, the game package to launch)
5. **The 24 JNI export symbols in `../src/android/mod.rs`.** JNI binds by
   *function name* - `Java_dev_marc_ce_overlay_NativeBridge_nativeInit` - so
   renaming the Java package orphans every one of them.
6. Changing `sharedUserId` changes the uid, so installed copies must go.

The rename is reversible: run it again with the old `--base`. It reads the
current names out of the project rather than assuming them, so it can be run
repeatedly.

### `./pack.sh` — build, verify, package

Covered above.

The `cargoNdk` task in `overlay/build.gradle.kts` runs
`cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 ... --no-default-features --features android`,
which is what keeps ratatui and crossterm out of the Android build entirely, and
drops the result into `overlay/src/main/jniLibs/`. `:cheatengine` picks it up
transitively, so nothing in that module knows Rust exists.

Override the toolchain locations with `-Pce.cargo=<path to cargo>` and
`-Pce.ndk=<ndk dir>` if they are not where the defaults expect.

## Walkthrough with the sample (embedded mode)

```bash
adb shell am start -n dev.marc.ce.sample/.MainActivity
```

1. Tap the **CE** bubble. Drag it anywhere; the app underneath keeps working.
2. **Scan** tab: `4 Bytes (u32)`, `Exact Value`, type the native counter's
   current value, **New Scan**. Expect a few thousand hits.
3. Press the app's **-10**. Switch the mode to **Decreased**, **Next Scan**.
   Repeat once or twice - the list collapses to a single address.
4. **Results** tab: tap the row to add it to the **Address** tab. Note that
   Results shows the value each address held *at scan time*; the Address tab is
   the one that reads live.
5. Edit the value there. The app's own display changes immediately.
6. Tick **F**. Now **-10** does nothing: a Rust thread rewrites the value every
   100 ms, independently of the UI and of any scan in flight.
7. **Hex** tab: paste the address to see the bytes change.

## Walkthrough with Dungeon Tap (standalone mode)

Gold is deliberately scarce - the shop costs far more than honest play earns for
a long while, which is the whole reason to go looking for it in memory.

1. Play a few waves. Note the gold.
2. Tap the **CE** bubble. The game keeps responding underneath - that is
   `FLAG_NOT_TOUCH_MODAL` doing its job, and the whole workflow depends on it.
3. `4 Bytes (u32)`, `Exact Value`, the current gold, **New Scan**.
4. Kill another monster. The reward is irregular, so "it went up by exactly N"
   is not a shortcut. **Next Scan** with the new gold, and repeat until the list
   is short.
5. Tap the row, then edit the value in the **Address** tab. Every shop button
   lights up at once. (Results shows scan-time values, so an address that has
   since been freed still displays its old number there - the Address tab reads
   live and will show it for what it is.)
6. Tick **F** and go shopping: the freeze thread rewrites the value every 100 ms,
   so buying costs nothing.
7. **Hex** tab on that address: `score`, `wave`, `attack` and `armor` are
   sitting right beside it in the same allocation, so the rest of the struct is
   free.
8. Watch `javaGold` drift away from `gold` while gold is frozen. Same number,
   two places: one in the buffer the engine is writing, one an ordinary Java
   field. **Force GC** makes the other half of that lesson - a moved object -
   happen on demand.

`HP` is the easier second target if you would rather not aim: it falls on its
own, so `Unknown Initial` followed by repeated `Decreased` narrows it with no
typing at all.

## Design notes worth knowing before you edit this

- **Two windows, not one.** The bubble is `FLAG_NOT_FOCUSABLE` so it never
  steals the IME or the back key. The panel clears that flag so text fields
  work - and *must* also set `FLAG_NOT_TOUCH_MODAL`, or it swallows every touch
  outside itself and the app you are inspecting stops responding.
- **Never make an overlay window full-screen.** Since Android 12 the system
  blocks touches passing through an overlay to the app below. Same-UID windows
  are exempt, which holds here, but a content-sized bubble and a bottom-anchored
  panel stay out of that argument entirely.
- **No AndroidX, no Kotlin, no XML.** Framework views only. Every runtime
  dependency this library carries is a version it could force on an arbitrary
  host app, and every resource it declares is a merge conflict.
- **No `Spinner`.** Its dropdown is a `ListPopupWindow` that inherits the
  anchor's window token and defaults to `TYPE_APPLICATION_PANEL`; inside a
  `TYPE_APPLICATION_OVERLAY` window that is a guaranteed `BadTokenException`,
  and there is no public hook to change the popup's window type. `ChoiceStrip`
  draws the choices inside the panel, so it adds no window and behaves
  identically under both hosts.
- **All durable state lives in Rust** (`../src/session.rs`). The Java side holds
  only view state, so an Activity being destroyed and recreated costs nothing
  but a re-attach.
- **`shutdown()` blocks and must not run on the main thread.** It joins the scan
  thread, and a scan across a whole process can take seconds. `shutdownAsync`
  cancels the scan first, removes the windows synchronously (WindowManager is
  main-thread-only) and joins on a worker.
- **No JNI is called from a Rust thread.** Scan and freeze threads touch only
  atomics and Rust data; Java polls. That removes the whole
  `AttachCurrentThread` class of crash.
- **Nothing may unwind into the JVM.** Every exported function is wrapped in
  `catch_unwind` (`../src/android/mod.rs`), so no profile in the Android build
  may ever set `panic = "abort"`.
