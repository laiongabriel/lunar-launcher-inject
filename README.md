# lunar_launcher_inject

Small wrapper for Lunar Client's Electron launcher.

Uses the [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
to override the node `child_process.spawn` function and inject JVM arguments. See [payload.js](src/payload.js).

Re-enables agent attaching and loads agents from an `agents` directory next to the executable:

* `.jar` files as premain agents using `-javaagent:`
* `.dll` (Windows), `.so` (Linux), or `.dylib` (macOS) files as native agents using `-agentpath:`

## Usage

* [Download the executable for your OS](https://github.com/laiongabriel/lunar-launcher-inject/releases/latest)
* Place it in a directory and put any premain agents you want to use inside an `agents` folder next to it
* Launch lunar using this program instead of the official launcher. The launcher looks normal but will silently inject the agents on launch.

Lunar is located automatically in:

* Windows: `%LOCALAPPDATA%\Programs\Lunar Client\Lunar Client.exe`
* macOS: `/Applications/Lunar Client.app` or `~/Applications/Lunar Client.app`
* Linux: `/usr/bin/lunarclient` (distribution packages such as the AUR one)

If Lunar lives somewhere else — the official Linux build is an AppImage, which has no fixed location — pass its path as the first argument:

```
lunar-launcher-inject "/path/to/Lunar Client.AppImage"
```

The release build runs without a console window. If the injection fails, the reason is written to `%TEMP%\lunar-launcher-inject.log`.

## Building

```
cargo build --release
```

## Credits

This repository is a **fork** of
[Nilsen84/lunar-launcher-inject](https://github.com/Nilsen84/lunar-launcher-inject),
by **Nilsen84**. The original work — the Chrome DevTools Protocol handshake, the
`child_process.spawn` override and the JVM argument injection — is his. If you
just want the original tool, use the upstream repository.

Changes in this fork:

* agents are loaded from an `agents` directory next to the executable instead of a fixed path;
* only the default Lunar installation location is searched, with the launcher path accepted as an argument;
* the Windows path is resolved from `%LOCALAPPDATA%`; macOS and Linux locations preserved;
* release builds run without a console window and log failures to `%TEMP%\lunar-launcher-inject.log`;
* cleanup: OS selection at compile time (`#[cfg]` instead of a runtime `match`), the CDP message dump was removed, the unused JavaScript parameter was dropped and the 15-second startup timeout is a single constant.
