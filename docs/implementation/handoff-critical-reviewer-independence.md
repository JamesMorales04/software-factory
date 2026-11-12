# Handoff — Critical reviewer independence without model diversity

**Estado:** handoff para el siguiente agente. No implementa ni certifica nada por sí
mismo. Baseline de código: rama `factory/agentic-flow-20261108`, candidato
`feff8ed`; el handoff se apoya en la documentación acumulada hasta `c19a251`.

## Corrección de política

La política actual confunde dos cosas:

- **Independencia requerida:** reviewers en procesos/sesiones separados, con
  identidades, tokens y principals autenticados distintos. Es lo que exige
  `ELEVATED`/`CRITICAL` y lo que el Gate ya puede comprobar con
  `COUNT(DISTINCT principal_id)`.
- **Diversidad de modelo/proveedor:** puede ser una señal de riesgo útil, pero
  **no es un requisito de independencia**. Dos reviewers con el mismo
  proveedor y modelo, en procesos y con identidades distintas, son
  independientes para esta política.

Por tanto, el fail-closed actual es correcto mientras solo exista **una**
identidad de reviewer, pero su motivo y su comprobación son incorrectos:

- `native/src/bin/core.rs:10977` devuelve `REVIEWER_DIVERSITY_REQUIRED` y habla
  de “two independent reviewer provider/model pairs”.
- `native/src/review_roles.rs:123` (`validate_critical_diversity`) exige dos
  pares provider/model distintos.
- `native/src/bin/core.rs:11750` colapsa cualquier plan con más de un reviewer a
  un único reviewer Functional, incluso para `ELEVATED`/`CRITICAL`.
- `compose.yaml:340` y `native/src/bin/compose_init.rs:41` provisionan un solo
  servicio/token/principal de reviewer.

El objetivo es: **misma política de independencia, sin exigir modelos ni
proveedores distintos**. Si hay suficientes identidades independientes, el plan
debe conservar los roles y el número de reviewers requeridos y el Gate debe
agregar sus recibos. Si no las hay, debe fallar cerrado con un código y mensaje
correctos.

## Estado actual relevante

- `ReviewerPlan::for_lane` en `native/src/planning/mod.rs` ya define los roles
  requeridos por lane/assurance.
- `required_reviewers` en `native/src/planning/route.rs` ya deriva el plan.
- `native/src/agentic.rs` ya crea `reviewer_count` y `reviewer_roles` para el
  participant plan.
- `native/src/db/quality.rs` ya cuenta reviewers distintos por
  `principal_id` (`COUNT(DISTINCT r.principal_id)`) y el Gate ya exige el número
  requerido de recibos.
- `native/src/review_roles.rs` ya genera assignments con un `principal_id`
  distinto por rol y valida evidencia; el problema es que el criterio de
  criticalidad exige diversidad de modelo/proveedor y el path activo no usa un
  fleet real.
- `native/src/bin/core.rs` reduce el plan a un reviewer cuando
  `reviewer_count > 1`; eso no debe ocurrir para lanes que requieren varios
  reviewers.

## Plan previo: waves y estado

El plan aprobado es
[`docs/implementation/quality-first-delivery-plan.md`](quality-first-delivery-plan.md).
La rama actual ya integra la mayor parte de las Waves 0–2. Este handoff **no
reabre** esos work packages: corrige WP9 y cierra la aceptación pendiente de
Wave 3.

| Wave | Work packages | Estado en esta rama | Ya hecho | Pendiente |
|---|---|---|---|---|
| **Wave 0** | WP0 — contratos, esquemas, migraciones | **Completado** | Contratos Rust/JSON, validación determinista, migraciones `0060`, `0061`, `0063`, `0064`, hashes exactos de plan/artefacto y atribución autenticada del owner | — |
| **Wave 1** | WP1, WP2, WP4, WP5, WP6, WP9, WP11 | **Parcial** | WP1: workflow durable, outbox, idempotencia, recuperación y fence reclaim/finish. WP2: route floors, prompts, Planner en todos los lanes, Spec Manager `FORMAL_SPEC`, artefactos, plan review y transiciones. WP4: broker atómico en la frontera de librería, evidencia y límites. WP5: SSE reconnect, sin deadline oculto de 300 s, políticas de reservas y reparación estructurada. WP6: snapshots content-addressed, exclusión de `target`, rechazo de symlinks/archivos inseguros y cleanup existente. WP11: replay, redacción, comparación y bounds. | WP9: flota real de reviewers con identidades/tokens/principals separados; este handoff. WP4: wiring HTTP/Compose del runtime broker. WP5: UAT live de long-turn/SSE cut y matriz de fallos. WP6: reconciliación de volúmenes huérfanos al arranque y chaos dedicado. WP11: vista de trace en Core/Office. |
| **Wave 2** | WP3, WP7, WP8, WP10, WP12 | **Parcial** | WP3: stack, arquitectura, documentación, skill packs con frescura y DoD. WP7: renderer OpenSpec canónico y validador CLI `1.16.1` con imagen. WP8: Finding Router, disposiciones durables, Gate bloquea hallazgos OPEN y resolve/refute autenticados. WP10: panel de plan review y disposición de hallazgos en Director. WP12: test golden del pipeline y suites Rust/Office existentes. | WP3: scheduler de refresh de skill packs y catálogo más amplio. WP7: job Compose de validación OpenSpec. WP8/WP10: aceptación browser real y revisión visual. WP12: matriz chaos/fault-injection y matriz browser por opción. |
| **Wave 3** | Integración y aceptación | **Pendiente** | Integración de rutas Core/Office, evidencia implementer y revisiones independientes Codex de WP0–WP10. | WP9 fleet, WP10 live wiring/acceptance, WP12 E2E/chaos completo, contract tests cross-workstream, candidato final, aceptación independiente y evidencia candidate-bound. |

**Orden de merges previsto:** WP0 → (WP1, WP2, WP4, WP5, WP6, WP9, WP11) →
(WP3, WP7, WP8) → WP10 → WP12. En esta rama ese orden ya está materializado en
los commits `0d7ef6e`, `96b43db`, `1b53b9a`, `7f17d59` y los fixes posteriores.
El trabajo restante se hace sobre el tip actual, sin reconstruir la rama.

**Alcance de este handoff:** cerrar WP9 (independencia por identidad/proceso,
no por modelo/proveedor) y dejar el camino de Wave 3 para aceptación real. No
declarar WP9 completo hasta que el fleet esté provisionado y probado; no
declarar Wave 3 completa sin browser/E2E/aceptación independiente.

## Plan de implementación

### 1. Corregir el contrato de independencia

En `native/src/review_roles.rs`:

- Sustituir `validate_critical_diversity` por
  `validate_reviewer_independence` (o nombre equivalente).
- Requisitos:
  - Al menos dos assignments para `CRITICAL`; el número exacto debe salir de
    `ReviewerPlan`.
  - `principal_id` únicos y no vacíos.
  - Ningún principal puede ser el implementer, el verifier ni otro rol del
    mismo reviewer.
  - Proveedor y modelo pueden ser iguales; no son criterio de rechazo.
- Si se quiere conservar diversidad de modelo como señal, añadir
  `model_diversity: Option<bool>` o una observación no bloqueante. No debe
  afectar la aceptación del plan ni del Gate.
- Tests:
  - Dos assignments, mismo provider/model, principals distintos → `Ok`.
  - Dos assignments, mismo principal → `Err`.
  - Un assignment para Critical → `Err`.
  - Implementer/verifier coludido → `Err`.

### 2. Provisionar identidades independientes

Elegir una de estas dos implementaciones, con preferencia por la primera:

**A. Fleet de servicios Compose (preferida).**

- Añadir servicios `reviewer-functional`, `reviewer-architecture`,
  `reviewer-security`, `reviewer-code-quality`, `reviewer-testing` y
  `reviewer-operability`, o un launcher que ejecute procesos separados.
- Cada servicio/proceso debe tener:
  - token propio, montado read-only desde su config volume;
  - `FACTORY_REVIEWER_PRINCIPAL` propio, por ejemplo
    `reviewer-functional-process`;
  - rol propio (`FACTORY_REVIEWER_ROLE`);
  - el mismo modelo/proveedor permitido.
- `native/src/bin/compose_init.rs` debe generar todos los tokens/principals y
  escribir cada uno en el volumen del servicio correspondiente. No compartir un
  token entre roles.
- `factory-reviewer` debe autenticarse con su token y enviar el `principal_id`
  que Core le asocie; Core no debe aceptar un principal elegido libremente por el
  caller.

**B. Registro de identidades en Core (si se mantiene un solo servicio).**

- Añadir configuración confiable, por ejemplo
  `FACTORY_REVIEWER_IDENTITIES_JSON`, con una lista de
  `{ token_file, principal_id, roles }`.
- Core autentica el token del reviewer y deriva el principal/rol; rechaza
  cualquier recibo cuyo `principal_id` no coincida.
- Un solo proceso puede rotar roles, pero cada recibo debe quedar ligado a una
  identidad autenticada distinta. Si se usa un solo proceso con varios tokens,
  documentarlo como limitación de aislamiento de proceso.

No añadir dependencias de host: tokens, configuración y procesos viven en
Compose. No exponer tokens en logs, UI ni telemetría.

### 3. Corregir el participant plan y el Gate

En `native/src/bin/core.rs`:

- Eliminar el colapso incondicional a un reviewer.
- Calcular:
  - roles requeridos: `required_reviewers(lane, assurance)`;
  - identidades disponibles: registro/fleet;
  - reviewers requeridos: número de roles/principals exigidos.
- Si `disponibles < requeridos`:
  - `CRITICAL`/`ELEVATED`: fallar cerrado con
    `REVIEWER_INDEPENDENCE_REQUIRED` y mensaje “critical/elevated assurance
    requires N independent reviewer identities/processes; provider/model
    diversity is not required”.
  - `DIRECT`/`DELEGATED`: aplicar la misma regla si su plan mínimo exige más de
    una identidad; no inventar cobertura.
- Si hay suficientes:
  - conservar `reviewer_count` y `reviewer_roles`;
  - asignar roles a principals distintos;
  - no reducir roles silenciosamente.
- El Gate en `native/src/db/quality.rs` debe seguir exigiendo el número de
  recibos y principals distintos. Añadir, si falta, la comprobación de cobertura
  de roles requeridos. Un recibo con el mismo principal dos veces no cuenta dos
  veces.
- Mantener el fail-closed actual como comportamiento transitorio hasta que el
  fleet esté disponible, pero con el motivo correcto.

### 4. Office y documentación

- En `office/src/views/Director.tsx` y `coreApi.ts`, mostrar roles,
  principals y número de recibos independientes. No mostrar “model diversity
  required”.
- Actualizar textos EN/ES.
- Actualizar:
  - `docs/architecture/decisions/ADR-0037-agent-topology-context-and-composition.md`
    o añadir una enmienda/ADR que defina independencia como identidad/proceso,
    no modelo.
  - `docs/implementation/quality-first-planning-status.md`.
  - `docs/product/office-parity.json`.
  - `docs/product/capabilities.yaml`.
  - `docs/roadmap/ROADMAP.md`.
  - `docs/evidence/quality-first-planning-2026-11-09/manifest.json` o un
    manifiesto nuevo del candidato.
- No reescribir ADRs aceptados; superseder o enmendar con fecha.

## Criterios de aceptación

1. `validate_reviewer_independence` acepta dos reviewers con el mismo
   proveedor/modelo y principals distintos.
2. Rechaza principals duplicados, un solo reviewer para Critical/Elevated, y
   colusión con implementer/verifier.
3. El participant plan conserva `reviewer_count` y roles cuando hay identidades
   suficientes; no colapsa a uno.
4. El Gate acepta un candidato con dos recibos PASS de principals distintos y
   el mismo modelo/proveedor; rechaza dos recibos del mismo principal; no acepta
   con un recibo cuando se requieren dos.
5. Critical/Elevated con una sola identidad devuelve
   `REVIEWER_INDEPENDENCE_REQUIRED` (no `REVIEWER_DIVERSITY_REQUIRED`) y el
   mensaje no menciona modelos/proveedores.
6. Compose provisiona al menos dos identidades de reviewer con tokens/principals
   distintos y `podman compose config` es válido.
7. Un E2E real (aunque sea con fixture provider) produce dos recibos
   independientes para un candidato Elevated/Critical y el Gate los agrega.
8. No hay secretos en logs/UI/telemetría; no se añaden dependencias de host; no
   se relaja el Gate ni la autorización.

## Comandos de verificación

Desde `native/`:

```text
cargo test -p factory-native --lib review_roles
cargo test -p factory-native --lib db::quality
cargo test -p factory-native --lib
cargo clippy -p factory-native --all-targets --all-features -- -D warnings
```

Desde la raíz:

```text
podman compose config
podman compose up -d --build
```

Office:

```text
npm test -- --run
npm run typecheck
npm run lint
```

## Restricciones

- **No** exigir proveedores o modelos distintos para la independencia.
- **No** aceptar un solo reviewer para `ELEVATED`/`CRITICAL`.
- **No** permitir que el caller elija el `principal_id` del reviewer.
- **No** compartir tokens entre roles.
- **No** afirmar cobertura multi-reviewer si el deployment no la tiene.
- **No** afirmar aislamiento o certificación `TRUSTED_LOCAL`.
- Mantener Core como autoridad; el modelo y los reviewers no aceptan trabajo.

## Formato de reporte

Reportar:

- Action/candidate SHA y archivos cambiados.
- Tests y comandos ejecutados, con resultados exactos.
- Evidencia real de Podman/Compose (no solo mocks).
- Hallazgos de reviewers y cómo se cerraron.
- Impacto de seguridad/autorización.
- Documentación actualizada.
- Riesgos y gaps restantes; distinguir `NOT_TESTED`, `BLOCKED`, `SKIPPED` y
  `NOT_APPLICABLE`.

## Prompt listo para copiar

```text
Trabaja en /home/james/Escritorio/Development/software-factory, rama
factory/agentic-flow-20261108, baseline e59c812 / código feff8ed.

Corrige la política de independencia de reviewers: la independencia requerida
es de identidad, proceso, token y principal autenticado, NO de modelo ni
proveedor. Dos reviewers con el mismo proveedor/modelo y principals distintos
son independientes.

Estado actual incorrecto:
- core.rs devuelve REVIEWER_DIVERSITY_REQUIRED y exige dos pares
  provider/model.
- review_roles.rs validate_critical_diversity exige provider/model distintos.
- core.rs colapsa el participant plan a un reviewer Functional.
- Compose provisiona un solo reviewer/token/principal.

Implementa:
1. validate_reviewer_independence basado en principals únicos, no en
   modelo/proveedor; tests con mismo modelo/proveedor y principals distintos.
2. Fleet de reviewers en Compose (o registro de identidades en Core) con
   tokens/principals/roles separados; mismo modelo permitido; sin secretos en
   logs/UI; sin dependencias de host.
3. Participant plan y Gate que conserven reviewer_count/roles y agreguen
   recibos de principals distintos; fail-closed con
   REVIEWER_INDEPENDENCE_REQUIRED si no hay suficientes identidades.
4. Office/docs/parity/capabilities/evidence actualizados.
5. Tests: dos reviewers mismo modelo/principal distinto -> Gate acepta; mismo
   principal dos veces -> rechaza; un reviewer para Critical/Elevated ->
   fail-closed; Compose config válido; E2E con dos recibos.

No relajes el Gate, no aceptes un solo reviewer para ELEVATED/CRITICAL, no
permitas principal elegido por el caller, no compartas tokens. Reporta SHA,
archivos, tests, evidencia Podman real y gaps restantes.
```
