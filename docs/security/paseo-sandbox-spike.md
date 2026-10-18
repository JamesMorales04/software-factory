# SEC-01 — Spike Paseo, sandbox, credenciales y observabilidad

**Estado:** plan de prueba; NO certificado. **Regla:** no ejecutar código no confiable con credenciales reales expuestas. **Alcance:** local-first CachyOS/Linux, Paseo daemon + @getpaseo/client y rootless Podman. El plan no implica que las topologías sean seguras.

## Preguntas de prueba

1. ¿Podemos descubrir modelos válidos, iniciar una sesión Paseo, correlacionarla con un Run, leer su timeline, cerrar/reconectar y recuperar eventos faltantes?
2. ¿En qué procesos, UID, mount namespace y entorno residen credenciales del daemon/proveedor y las herramientas ejecutadas por agentes?
3. ¿La ejecución no confiable puede leer un **canary** fuera del workspace, sockets del host, credenciales de prueba o datos de otro proyecto? ¿Puede modificar fuera de su espacio?
4. ¿Funciona la ejecución de un agente con el aislamiento exigido **sin** desmontar restricciones o compartir secretos con el código de la tarea?
5. ¿Qué cambia para Codex vs OpenCode y sus modes/options? No asumir equivalencia de políticas.

## Puertas y fases

- **A. Inventario sin ejecutar agentes:** `command -v paseo`, `paseo --version`, `paseo ls`, `paseo provider diagnostic codex --json`, `paseo provider diagnostic opencode --json`, versiones Podman y Node.js. Nunca publicar credenciales ni dumps completos de entorno.
- **B. Prueba negativa de sandbox sin credenciales:** imagen fija, `--network none`, read-only, `--cap-drop all`, `no-new-privileges`, UID no privilegiado, limites CPU/RAM/PIDs, un solo bind ro del fixture. Probar que puede leer fixture, pero no escribir, acceder a canary externo, red, socket ni otro proyecto. Registrar resultados con código de salida. Un PASS aquí NO certifica Paseo.
- **C. SDK Paseo en repo inocuo:** Node >=22, `@getpaseo/client` fijado. Descubrir un modelo `ready` por `providers.waitForReady`, iniciar agente en repo desechable, inspeccionar `waitForFinish`, timeline y reconexión. Nunca utilizar código no confiable en el host autenticado.
- **D. Frontera crítica de credenciales:** con canaries falsos, verificar dónde se ejecutan los comandos/herramientas de agentes. Contrastar: (i) daemon y provider ambos en host; (ii) daemon/provider dentro de sandbox; (iii) daemon confiable con ejecutores no confiables en contenedor separados **solo si** las opciones de proveedor permiten imponer la separación. Observar aislamiento del proceso **real**, no inferirlo de los prompts. Si el agente puede leer canaries fuera del workspace o acceder a sockets privilegiados, FAIL. Si el provider necesita credenciales en el mismo dominio que comandos no confiables, bloquear esa topología o limitarla a un entorno y credenciales desechables de mínimo privilegio expresamente aceptado.
- **E. Finalización y recuperación:** detener/reanudar/reconectar, observar eventos, garantizar no doble efecto. SDK timeline live-only tras reconexión: realizar `refetch` por cursor/epoch/seq para recuperar historial; una sustitución de epoch invalida el cursor anterior.

## Aceptación

`SEC-01` solo pasa con evidencia verificable para **ambas** propiedades: funcionalidad SDK real y separación efectiva de credenciales/herramientas. Debe adjuntar runtime/versiones, IDs de sesión (sin secretos), comando y opciones efectivas de sandbox, montaje real, árbol de procesos anonimizado, pruebas positivas y negativas, logs redactados, diagnóstico de riesgos y decisión `PASS|FAIL|BLOCKED`. Controles de SDK y aislamiento pueden tener resultados diferentes; no fusionarlos en un PASS.

## Recursos oficiales

- https://paseo.sh/docs/sdk/quickstart
- https://paseo.sh/docs/sdk/providers
- https://paseo.sh/docs/sdk/events
- https://paseo.sh/docs/providers
- https://paseo.sh/docs/docker
- https://docs.podman.io/

**No usar:** `--privileged`, `--security-opt label=disable` sin justificación específica, mounts de `$HOME`, directorios `.codex`/`.config/opencode`, `podman.sock`/`docker.sock` ni SSH agent dentro del sandbox no confiable; tampoco `--network host`. No deshabilitar sandbox para declarar cierre.
