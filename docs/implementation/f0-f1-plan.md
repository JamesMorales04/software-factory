# Plan de inicio: F0 y F1 — validación continua con uso real

> **Estado:** propuesta para ratificación antes de implementar. Ningún repo o código de producto fue creado mediante este paquete.

## F0 — Documentation first

**Entrada:** decisiones de producto ya acordadas. **Salida:** primer commit documental del repositorio *después de autorización explícita*, con README, AGENTS, MASTER, ADR, contratos F0, calidad, seguridad, telemetría y plan de pruebas. Nombre definitivo y licencias por decidir.

F0-01. Revisar MASTER, marcar `Acordado / Propuesto / No verificado`; resolver inconsistencias.

F0-02. Especificar seis contratos: Project–Action, Campaign–Run, Core–Worker, Agent Session, Quality–Delivery y Factory Events; cada uno con estados, idempotencia, permisos, errores y tests.

F0-03. Diseñar prueba de viabilidad bloqueante para Paseo + sandbox + tokens de proveedor + inspección de sesiones. Ejecutarla **después** de autorizar el trabajo técnico en un entorno descartable, antes de desarrollar dependencias extensas.

F0-04. Establecer comandos reales de build/test solo cuando exista código. Configurar comprobación de docs/enlaces y secret scanning en CI del futuro repo.

## F1 — vertical slices; no avance sin evidencia

| Slice | Entrega | Prueba negra de usuario | Prueba negativa o de fallo |
|---|---|---|---|
| S1 | Core persistente + API + UI mínima + eventos/OTel | Arrancar servidor, abrir UI, crear/ver Action, reiniciar y ver persistencia | Denegar operaciones inválidas, evitar evento duplicado, OTel caído no bloquea |
| S2 | Worker local y contrato Core–Worker | Crear Run, ver Worker real y resultado | Crash/reconnect/lease, reenvío, no doble ejecución |
| S3 | Paseo real + aislamiento viable | Crear sesión, ejecutar tarea y observarla por UI | Host/otro proyecto/secretos/sockets no accesibles, red acotada |
| S4 | Implementer, Reviewer y Quality Baseline | Producir un diff real, tests y revisión independiente | Falla obligatoria bloquea; reviewer no modifica candidato |
| S5 | GitHub Delivery | PR real, CI y evaluación sobre commit exacto | PR R0 no elegible bloqueado; no doble merge, SHA modificado revalida |
| S6 | Campaña de autodesarrollo | Core estable A desarrolla mejora real de UI en candidato B y entrega por PR | B no altera A; estados correctos en UI; rollback de candidatos sin afectar Core |

Las características R0 de merge automático deben probarse con un PR real elegible; el PR de autodesarrollo solo se fusionará cuando su riesgo final y permisos lo permitan.

## Algoritmo de trabajo para cada slice

1. Preparar un criterio de aceptación *observable desde fuera* antes de programar; congelar los tests de aceptación requeridos.
2. El agente implementa el cambio más pequeño dentro de su workspace.
3. Ejecuta unit/contract/integration pertinentes y arranca **el producto construido** en un entorno desechable.
4. Implementer realiza comprobaciones propias como usuario para localizar fallos y repararlos. Captura salida y trazas pertinentes.
5. **Independent Verifier** (etapa Review, distinto del autor) toma el commit exacto en un checkout limpio y realiza los recorridos negros de usuario con oráculos no editables por el Implementer: CLI, HTTP, UI Playwright y/o Paseo real según el slice.
6. Si falla: reproducir, diagnosticar con eventos/traces, abrir/fijar Repair subtask, corregir, revalidar pruebas impactadas (y la integración afectada). Iteración acotada por progreso y presupuesto.
7. Reviewer independiente inspecciona candidato, seguridad y evidencia; Quality Gate verifica contratos sin aceptar autoafirmaciones del Implementer.
8. PR/merge según autoridad. No fusionar `NOT_TESTED`, PASS parcial, evidencias de SHA obsoleto o scripts que no se ejecutaron.
9. Solo después de aceptación pasar al siguiente slice. Fallos encontrados posteriormente se convierten en Incidents/Actions y casos de regresión.

## Evidencia mínima por slice

`sliceId`, `actionId`, `commit`, `candidateDigest`, `policyRevision`, `runId`, `workerId`, `paseoSessionId` (si aplica), comando/recorrido de usuario, resultados de checks, logs redactados, trazas/capturas si corresponden, errores y reparaciones, reviewer, dictamen Gate, URL/recibo de PR si aplica. La evidencia se guarda con retención y accesos, no se vuelca íntegramente al repo.

## Riesgos bloqueantes y salida

F1 no se declara completa con mocks de Paseo, credenciales expuestas, sandbox degradado, UI simulada, inexistencia de prueba real, CI saltado, merge no autorizado ni Run irreconciliable. Las pruebas de seguridad y recuperación son parte de la aceptación, no tareas pospuestas.

## Próximos pasos concretos

1. Validar este paquete documental con el propietario; decidir nombre/repositorio/licencia y visibilidad.
2. **Solo tras autorización:** crear repositorio, incorporar estos documentos en primer commit, abrir PR de ratificación de arquitectura/contratos.
3. Ejecutar spike de seguridad Paseo antes de invertir en el Worker completo.
4. Implementar S1 con telemetría desde el arranque; prueba de navegador real.
5. Avanzar slice a slice sin permitir que el Implementer sea su propio oráculo.

## Preparación técnica de F0

Antes de fijar Go o Rust como lenguaje definitivo, estudiar `docs/architecture/decisions/ADR-0005-go-vs-rust-pending.md`. Para el bloqueo de Paseo+credenciales ejecutar `docs/security/paseo-sandbox-spike.md`. Prompt de coordinación: `docs/implementation/campaign-prompt.md`.
