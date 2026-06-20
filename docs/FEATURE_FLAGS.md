# Feature flags

Por device, server expõe:

```
GET  /api/v1/devices/:id/manifest
PUT  /api/v1/devices/:id/features    (admin)
```

Defaults:
| flag            | default  |
|-----------------|----------|
| catalog         | enabled  |
| cloud_save      | planned  |
| netplay         | locked   |
| runtime_probe   | disabled |
| hardware_tests  | enabled  |
| beta_features   | locked   |
| community       | enabled  |

Agent:
```
playora-agent features fetch       # baixa manifest e grava no SQLite local
playora-agent features show
```

Para habilitar runtime probe num device específico de testes:
```
curl -X PUT http://<server>/api/v1/devices/<dev_id>/features \
  -H 'content-type: application/json' \
  -d '{"runtime_probe":"enabled"}'
```
