# Dados de hardware coletados

Em runtime, no R36S, o agent lê:

- /proc/cpuinfo → modelo, cores, "Hardware:" string
- /proc/loadavg → load 1/5/15
- /proc/meminfo (via sysinfo) → total/avail/swap
- /proc/mounts + statvfs → disks
- /sys/class/thermal/thermal_zone*/{type,temp} → temperaturas nomeadas
- /sys/devices/system/cpu/cpu*/cpufreq/{scaling_cur_freq,scaling_governor}
- /sys/class/power_supply/* → baterias
- /sys/class/net/* → interfaces + MAC (hash sha256, nunca puro)
- /sys/bus/usb/devices → vendor/product/product-name
- /sys/class/graphics/fb0/name → framebuffer
- /proc/asound/cards → audio
- /dev/input/event* → input devices (listagem, sem leitura ativa)
- /proc/device-tree/dsi@ff450000/panel@0/* → painel (compatible, timings)
- `command -v retroarch` → detecção
- `retroarch --version`

Saída unificada em `HardwareSnapshot` (JSON estável, serde).

Privacidade:
- MAC nunca cru — só hash.
- SSID não é coletado.
- Sem leitura de RAM do jogo (runtime_probe = disabled por padrão).
