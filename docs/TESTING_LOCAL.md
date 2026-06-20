# Testing local (Mac / Linux dev box)

```sh
# 1) server
cargo run -p playora-server -- --db ./server.db --bind 0.0.0.0:8080
# in another shell:
curl http://127.0.0.1:8080/health   # -> ok

# 2) agent
cargo run -p playora-agent -- init --server-url http://127.0.0.1:8080
cargo run -p playora-agent -- doctor
cargo run -p playora-agent -- hardware snapshot
cargo run -p playora-agent -- heartbeat
cargo run -p playora-agent -- test-session --system snes --game "Fake SNES Test" --duration 5
cargo run -p playora-agent -- sync

# 3) dashboard
open http://127.0.0.1:8080/dashboard
```

Em `hardware snapshot` rodando no macOS muitos campos do Linux retornam `null` (esperado).
No R36S retornam o real.
