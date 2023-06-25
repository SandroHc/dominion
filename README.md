# Dominion

A URL watcher that notifies you of any changes.

~~A 100% free domain monitor.~~

## Building for Debian

1. `cargo install cargo-deb`
2. `rustup target add x86_64-unknown-linux-musl` (while on Windows; musl is more portable)
3. `cargo deb --target x86_64-unknown-linux-musl`
4. Install the package with `dpkg -i target/debian/*.deb`
5. Inspect the package with `dpkg -e target/debian/*.deb` to inspect the systemd scripts
6. Update the config file "/home/$USER/.config/dominion/dominion.toml" and restart the service via `systemctl restart dominion.service`
7. And enable the service if not already enabled: `systemctl enable dominion.service`. This will start the service on host startup.

### Useful links

1. https://www.ebbflow.io/blog/vending-linux-1
2. https://www.ebbflow.io/blog/vending-linux-2
