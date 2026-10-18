# AGENTS.md — Software Factory

> **Borrador para el futuro repositorio.** Este archivo es un punto de entrada breve para cualquier agente. El estado de las capacidades debe comprobarse en código y evidencias, nunca inferirse del roadmap. Nombre del proyecto provisional.

## Misión

Construir una Software Factory multiproyecto, local-first, capaz de desarrollar y reparar su propio software con agentes reales, aislamiento, calidad verificable, uso eficiente de recursos y control de autoridad por el Core.

## Antes de trabajar

1. Leer `docs/README.md` y `docs/architecture/MASTER.md`. Consultar los contratos y ADR pertinentes a la Action asignada; no cargar todo el repositorio innecesariamente.
2. Identificar la Action, especificación **vigente/fijada**, alcance, commit inicial, política, permisos y criterios de aceptación. Si faltan, solicitar una aclaración por la vía autorizada; no inventar autoridad.
3. Comprobar el estado real del código y las herramientas. No asumir que un componente documentado está implementado o integrado.
4. Respetar el sandbox y el workspace asignados. No tocar el Core estable, otros proyectos, HOME del host, credenciales, sockets privilegiados ni datos fuera del alcance.

## Principios invariantes

- El Core determinista posee estado, autorización, Quality Baseline, presupuesto y efectos externos. Las respuestas de LLM son propuestas/evaluaciones, no evidencia de autorización.
- No sustituir pruebas reales por mocks en la aceptación final; los mocks sirven para unidades, no para acreditar recorridos de usuario.
- No declarar PASS, ACCEPTED, MERGED o DEPLOYED sin su evidencia exacta. Distinguir NOT_TESTED, BLOCKED y NOT_APPLICABLE.
- No ampliar un contrato, alterar una spec fijada, degradar pruebas obligatorias ni extender presupuestos para conseguir pasar una validación.
- Reviewer independiente del Implementer. El autor ejecuta pruebas de desarrollo y repara; un Independent Verifier de la etapa Review ejecuta recorridos negros desde el commit exacto en un entorno limpio. Reviewer inspecciona evidencia y Quality Gate certifica. El Verifier/Reviewer no puede escribir sobre el candidato.
- No introducir fallbacks no autorizados a procesos sin sandbox, modelos incompatibles o permisos más amplios.
- Crear el cambio mínimo que cumple el objetivo. No ampliar scope ni añadir dependencias especulativas.
- No registrar tokens, prompts completos, variables de entorno sensibles ni código fuente innecesario en telemetría.
- Solo utilizar skills admitidas por el procedimiento y capacidades del Run. Una skill externa no concede derechos.

## Flujo de desarrollo obligatorio

1. Reproducir la tarea/fallo cuando sea posible y registrar la condición inicial.
2. Proponer el cambio mínimo; documentar decisiones arquitectónicas mediante ADR cuando corresponda.
3. Implementar y ejecutar pruebas deterministas relevantes.
4. **Usar el producto de forma real:** arrancar binarios/servicios, invocar CLI/API y/o navegar UI con Playwright según el incremento. No limitarse a probar funciones aisladas.
5. Ante fallo: guardar evidencia, diagnosticar y reparar dentro del presupuesto. Repetir la prueba y la regresión pertinente.
6. Actualizar código, pruebas, documentación y telemetría afectadas en el mismo PR.
7. Congelar el candidato; un Independent Verifier distinto del autor ejecuta los recorridos reales de aceptación sobre un checkout limpio, sin editar el candidato ni sus oráculos.
8. Reviewer independiente revisa calidad, seguridad y evidencias. El Core decide aceptación y delivery.

## Documentación y cambios de arquitectura

- `docs/architecture/MASTER.md`: objetivo, estructura y límites.
- `docs/architecture/decisions/`: decisiones y alternativas; nunca reescribir una decisión aprobada para cambiarla: registrar ADR sustituto.
- `docs/contracts/`: interfaces y protocolos normativos.
- `docs/quality/baseline.md`: controles globales y aceptación.
- `docs/observability/telemetry.md`: eventos, trazas, métricas y redacción de datos.
- `docs/implementation/f0-f1-plan.md`: hitos, bloqueadores y pruebas.
- `docs/security/paseo-sandbox-spike.md`: plan bloqueante SEC-01.
- `docs/implementation/campaign-prompt.md`: prompt de bootstrap de campaña, sin nuevas autorizaciones.

Un cambio de arquitectura significativo exige Action/ADR, actualización de referencias y pruebas pertinentes. Un cambio a seguridad, política de merge, Quality Baseline o control de upgrades no es R0 ordinario.

## Comandos

**No hay comandos de build/test autorizados todavía: F0 es documental y la implementación no ha empezado.** Al comenzar F1, añadir únicamente comandos que realmente existan y estén certificados; no dejar instrucciones ficticias.

## Informe de trabajo esperado

Comunicar: qué se cambió; Action/commit; pruebas y recorrido real ejecutados; evidencia; discrepancias o fallos; seguridad/permisos; efecto en documentación; estado de entrega. No redactar conclusiones que excedan la evidencia.
