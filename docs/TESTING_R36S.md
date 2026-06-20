# Testing no R36S real

```sh
# pelo SSH no R36S:
playora-agent doctor
playora-agent hardware snapshot --save
playora-agent hardware test --mode quick
playora-agent heartbeat
playora-agent test-session --system snes --game "R36S Test" --duration 5
playora-agent sync

# ou pela ES:
# Menu Ports -> Playora Status / Sync Now / Doctor / etc.
```

Confirma no dashboard do server (Mac):
```
http://<IP_DO_MAC>:8080/dashboard
```

Devem aparecer:
- devices = 1
- events = vários
- sessions = 1+
- ranking populado
