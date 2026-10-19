# lunar_launcher_inject
Small wrapper for lunar clients electron launcher.

Uses the [Chrome DevTools Protocol](https://chromedevtools.github.io/devtools-protocol/)
to override the node child_process.spawn function and inject jvm arguments. See [payload.js](src/payload.js).

Re-enables agent attaching and loads agents from an `agents` directory next to the executable:

* `.jar` files as premain agents using `-javaagent:`
* `.dll` (Windows), `.so` (Linux), or `.dylib` (macOS) files as native agents using `-agentpath:`

## Usage
* [Download the executable for your OS](https://github.com/laiongabriel/lunar-launcher-inject/releases/latest)
* Place it in a directory and put any premain agents you want to use inside an `agents` folder next to it
* Launch lunar using this program instead of the official launcher. The launcher looks normal but will silently inject the agents on launch.