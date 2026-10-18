# Índice de documentación — Software Factory

## Qué leer

1. [Arquitectura maestra](architecture/MASTER.md) — visión, límites, decisiones, riesgos y fases.
2. [Contratos](contracts/README.md) — interfaces y garantías entre componentes (por completar en F0).
3. [Quality Baseline](quality/baseline.md) — condiciones universales de aceptación.
4. [Seguridad y aislamiento](security/isolation.md) — límites de autoridad, credenciales y sandbox.
5. [Telemetría](observability/telemetry.md) — estado, eventos, trazas, logs, métricas.
6. [Factory Office](product/factory-office.md) — UX mínima y proyecciones.
7. [F0/F1](implementation/f0-f1-plan.md) — hitos y evaluación real de cada incremento.
8. [ADRs](architecture/decisions/README.md) — decisiones arquitectónicas, motivos y sustituciones.

## Jerarquía de referencias

- **Propietario del producto:** aprueba decisiones y excepciones reservadas.
- **ADR aceptado:** razón y alcance de una decisión arquitectónica acordada; si se sustituye, registrar un ADR nuevo.
- **MASTER:** resumen normativo, acuerdos, límites y mapa de módulos; debe sincronizarse con ADR y contratos.
- **Contratos y políticas versionadas:** comportamiento y verificación concretos. Una autorización operacional solo proviene de la política exigible del Core, nunca del texto de un agente ni de un documento ambiguo.
- **AGENTS.md:** instrucciones operativas, no duplicación de toda la arquitectura.
- **Planes y notas:** propuestas o guías, no pruebas de integración ni concesiones de autoridad.

Ante contradicción documental: **detener afirmaciones de conformidad**, registrar problema y resolverlo mediante Action/ADR según riesgo. No elegir silenciosamente la alternativa menos restrictiva.

## Política de mantenimiento documental

- Cada cambio funcional debe revisar qué documentación, pruebas, diagramas y telemetría afecta; si no cambia la arquitectura, registrar explícitamente “sin impacto arquitectónico” en el PR.
- Cada decisión arquitectónica relevante sigue: Action → ADR propuesto → evaluación/riesgo → decisión autorizada → actualización conjunta de MASTER, contratos, tests y diagramas → revisión y PR.
- ADR aceptado se mantiene como historial: para reemplazarlo, crear nuevo ADR y marcar el anterior `Superseded`.
- Cambios de políticas de seguridad, aislamiento, merge, Quality Baseline o upgrade requieren vía protegida.
- La documentación debe indicar: `Acordado`, `Propuesto`, `No verificado`, `Implementado` o `Verificado`; no promocionar estados por existir un checkbox o prueba unitaria aislada.
- Documentos y contratos deben estar enlazados en el índice; comprobar enlaces y referencias en CI cuando comience la implementación.

## Documentación operativa generada

Las trazas, resultados de tests, capturas, logs y evidencias de Runs son **artefactos**, no fuente normativa de arquitectura. Deben tener identidad, retención y acceso controlados; no añadirse masivamente a `docs/` ni al historial Git. El repositorio conserva referencias y fixtures pequeños, nunca credenciales ni volúmenes completos de telemetría.

## Nuevos documentos de F0

- [`security/paseo-sandbox-spike.md`](security/paseo-sandbox-spike.md): prueba bloqueante de Paseo/sandbox/credenciales.
- [`architecture/decisions/ADR-0004-independent-user-journey-verification.md`](architecture/decisions/ADR-0004-independent-user-journey-verification.md): verificador independiente en la etapa Review.
- [`architecture/decisions/ADR-0005-go-vs-rust-pending.md`](architecture/decisions/ADR-0005-go-vs-rust-pending.md): comparación de lenguajes pendiente.
- [`implementation/campaign-prompt.md`](implementation/campaign-prompt.md): prompt de campaña inicial.
