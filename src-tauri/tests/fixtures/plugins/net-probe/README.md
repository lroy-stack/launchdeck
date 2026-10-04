# net-probe

A plugin for LaunchDeck's own tests, built with the public SDK. It makes whatever request a test asks for and returns what the host answered, so the tests in `src-tauri/tests/plugin_network.rs` can check the network capability from inside the sandbox. Its manifest is written by each test, so one module serves every case.

It is not an example to copy and it is not installed anywhere. Rebuild `net-probe.wasm` after changing it or the SDK:

    sh scripts/build-example-plugins.sh net-probe
