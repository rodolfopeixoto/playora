# Instalar agent no dArkOSRE-R36

1. Build cross aarch64 no dev box:
```sh
sh scripts/build-arm64.sh        # gera dist/playora-agent-aarch64
```

2. No R36S: ativa SSH em Options → Network → SSH ON (user `ark`/pass `ark`).

3. No dev box, copie e instale:
```sh
scp dist/playora-agent-aarch64 ark@<IP_DO_R36S>:/tmp/
ssh ark@<IP_DO_R36S> 'sudo mv /tmp/playora-agent-aarch64 /usr/local/bin/playora-agent && \
  sudo chmod +x /usr/local/bin/playora-agent && \
  playora-agent init --server-url http://<IP_DO_TEU_MAC>:8080 && \
  playora-agent doctor'
```

4. Adicionar menu em EmulationStation (Ports/Tools):
```sh
ssh ark@<IP_DO_R36S> 'sudo sh /tmp/install-darkosre-menu.sh'
# antes copia o script: scp scripts/install-darkosre-menu.sh ark@<IP>:/tmp/
```

5. Reinicia EmulationStation. Atalhos `Playora *` aparecem em **Ports**.
