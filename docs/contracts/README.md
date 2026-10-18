# Contratos de F0 — índice de trabajo

**Estado:** propuesto, pendiente de especificación y revisión. Esta lista no equivale a contratos completados.

| Contrato | Entidades/operaciones | Fallos obligatorios que debe cubrir |
|---|---|---|
| Project–Action | identidad, spec, estado, riesgo | duplicación, drift, cancelación, cambio de spec |
| Campaign–Run | dependencias, intento, presupuesto, ownership | reintentos, interrupción, leases, concurrencia |
| Core–Worker | registro, asignación, heartbeat, resultado, cancel | pérdida de red, replay, fencing, muerte del worker |
| Agent Session | modelo/runtime, workspace, eventos, permisos | no inicio, pérdida de sesión, timeout, incompatibilidad |
| Quality–Delivery | candidato, verificaciones, revisión, PR/merge | CI pendiente, SHA cambiado, webhook repetido, riesgo elevado |
| Factory Events | evento persistente, secuencia, stream/replay, RBAC | reconexión, duplicados, omisiones, filtrado de secretos |

Cada contrato deberá definir: `version`, propietario, entradas, salidas, identificadores, invariantes, estados, idempotencia, límites de seguridad, errores estructurados, observabilidad, pruebas positivas y negativas, y compatibilidad. No inventar RPC concretos antes de la prueba técnica de F0.
