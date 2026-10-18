# Factory Office — interfaz propia mínima de F1

> **Estado:** UI mínima acordada; representación visual animada posterior.

## Vistas F1

| Vista | Función |
|---|---|
| Dashboard | Core/Worker/salud, colas, campañas, estado de mantenimiento |
| Actions | Crear, consultar, filtrar Actions; mostrar spec y estado |
| Agents | Participantes reales con rol, runtime/modelo, estado y sesión Paseo |
| Action Detail | Timeline, Runs, herramientas observadas, pruebas, Review, PR/merge y bloqueos |

La UI recibe snapshots y eventos desde Core; no usa direct writes a DB ni controla directamente Workers. Reconecta por cursor/secuencia y recupera eventos faltantes. Estados desconocidos deben mostrarse `UNKNOWN/DISCONNECTED`; no deducir pensamientos internos del modelo. Poder inspeccionar no equivale a poder controlar. Renderizar contenido de agentes como datos no confiables, no comandos/UI con privilegios.

## Integración Paseo

Mantener asociación persistida de `runId`, `agentId`, `paseoSessionId`, `workerId`. Visualizar estado real y abrir detalle de inspección a través de ruta autenticada. Validar si el SDK permite timeline/replay y aislamiento de acceso; no asumir enlaces externos o capacidades inexistentes.

## Aceptación F1

- El navegador real crea una Action, ve cambios tras recargar y observa un Run activo real.
- El usuario navega Agents → Action Detail → evidencia, sin perder correlación.
- Desconectar y reconectar UI no pierde transiciones ni crea duplicados visibles.
- Acciones de gestión respetan autorización; vista read-only no eleva permisos.
- Playwright aporta evidencias de interacción real (trazas, capturas, consola y red) según políticas de redacción.

La oficina estilo pixel-art futura comparte los mismos eventos y no crea autoridad o estado paralelo.
