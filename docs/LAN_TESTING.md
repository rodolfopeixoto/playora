# LAN Testing — Mac server + R36S agent (same Wi-Fi)

## 1. Descobre IP do Mac na rede

```sh
ipconfig getifaddr en0     # Wi-Fi típico
ipconfig getifaddr en1     # ethernet
```

Resultado exemplo: `192.168.3.82`. Vamos chamar de `$MAC_IP`.

## 2. Sobe server escutando em todas as interfaces

```sh
./target/release/playora-server --db ./server.db --bind 0.0.0.0:8080
```

Acessível em:
- localhost: `http://127.0.0.1:8080`
- LAN: `http://<MAC_IP>:8080`
- Dashboard: `http://<MAC_IP>:8080/dashboard`

## 3. Firewall macOS

Se primeira execução, macOS pode pedir permissão de rede. Aceite.

## 4. Pelo R36S (após dArkOSRE + SSH ativados)

```sh
ssh ark@<R36S_IP>
playora-agent init --server-url http://<MAC_IP>:8080
playora-agent doctor                # confirma server reachable
playora-agent hardware snapshot --save
playora-agent heartbeat
playora-agent test-session --system snes --game "R36S LAN Test" --duration 5
playora-agent sync
```

## 5. Verifica no Mac dashboard

Abre `http://<MAC_IP>:8080/dashboard`. Deve mostrar:
- Devices: 1
- Events: 4+ (snapshot + heartbeat + 2 session events)
- Sessions: 1
- Ranking: "R36S LAN Test" listado

## 6. Upload de saves (tarball gz, metadata + bytes)

```sh
playora-agent saves upload
# server grava em $PLAYORA_SAVES_DIR/<device_id>/saves_<ts>.tar.gz
```

## 7. Trocar pra servidor de produção depois

Edita `/roms/playora/agent.toml`, altera `server_url`. Restarta agent.
