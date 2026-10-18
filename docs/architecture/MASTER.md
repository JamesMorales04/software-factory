# Software Factory — Especificación arquitectónica maestra

> **Estado:** Borrador de F0 para revisión. **Fecha:** 2026-10-18. **Versión documental:** 0.1.0.
> **Nombre del producto:** provisional. **Implementación:** no iniciada. Este documento captura decisiones de producto ya acordadas y propuestas técnicas todavía por verificar.

## 1. Propósito

Construir una Software Factory autónoma, multiproyecto, centrada en la entrega real de software de alta calidad. Debe convertir objetivos e incidentes en Actions y campañas; componer agentes con modelos y competencias adecuados; ejecutar código en entornos aislados; validar resultados independientemente; entregar por GitHub bajo políticas explícitas; y desarrollar su propio código mediante una versión controladora estable. El éxito se mide por resultados aceptados, fiabilidad, tiempo y recursos **observados**, no por número de sesiones, commits o PR generados.

### 1.1 Prioridades
1. Seguridad, aislamiento, integridad y autoridad correctamente delimitada.
2. Cumplimiento verificable de requisitos, Quality Baseline y entrega correcta.
3. Recuperación, observabilidad y continuidad autónoma.
4. Eficiencia: tiempo, tokens, recursos y costo total por resultado aceptado.
5. Interfaz propia, clara y observable, con una oficina visual como evolución.

### 1.2 Fuera del alcance inicial
Un motor de codificación propio, sustitución de Paseo, arquitectura de microservicios, cluster HA, workers remotos operativos, promoción automática de versiones del Core, oficina animada avanzada, optimización experimental en línea sin evaluación y dependencia obligatoria de SaaS para observabilidad.

## 2. Lenguaje normativo y madurez

- **Acordado:** decisión tomada por el propietario del producto; no cambiar sin nueva decisión y ADR cuando corresponda.
- **Propuesto:** dirección recomendada, pendiente de aceptación o prueba de factibilidad.
- **No verificado:** afirmación técnica que requiere prueba real.
- **Implementado:** existe código, pero no necesariamente integración certificada.
- **Verificado:** supera un caso de aceptación con evidencia reproducible en su superficie real.

**La documentación describe la intención normativa; la ejecución y la autorización se rigen por políticas y comprobaciones del Core, no por afirmaciones de un agente ni por instrucciones obtenidas de repositorios externos.** Si código, docs y política divergen, bloquear la afirmación de conformidad y abrir una Action para reconciliarlos.

## 3. Decisiones acordadas

| Dominio | Decisión | Estado |
|---|---|---|
| Producto | Factory central e independiente, multiproyecto | Acordado |
| Despliegue | Local primero; preparada para workers remotos propios/cloud administrado | Acordado |
| Runtime | Paseo como runtime inicial e inspector; UI propia posteriormente | Acordado |
| UI | UI web propia mínima obligatoria en F1 | Acordado |
| Aislamiento | Sandbox por defecto; acceso al host solo por excepción explícita | Acordado |
| Autonomía | Mantenimiento autónomo permanente | Acordado |
| GitHub | Merge autónomo condicionado para cambios R0 comprobados | Acordado |
| Skills | Descubrimiento, evaluación, adopción de scripts y promoción global autónomos sujetos a controles | Acordado |
| Agentes | Roles estables, especialización dinámica y composición mínima | Dirección acordada |
| Modelos | Routing adaptativo; promoción de rutas de bajo riesgo tras evals reproducibles | Acordado |
| Recursos | Reservas Development/Maintenance/Evaluation y préstamo dinámico | Acordado |
| Calidad | Baseline global obligatorio ampliado por perfiles de proyecto | Acordado |
| Autodesarrollo | Controlador estable aislado de la versión candidata | Requisito de seguridad aceptado |

### 3.1 Selecciones técnicas propuestas (no ratificadas)
- **Core:** monolito modular Go, proceso autónomo de Worker.
- **Persistencia:** SQLite con WAL para el Core local; nunca acceso directo de Workers remotos a la base.
- **Integración Paseo:** adaptador TypeScript que encapsule `@getpaseo/client`/CLI según funciones *efectivamente comprobadas*.
- **UI:** React/TypeScript, API HTTP y stream de eventos desde el Core (SSE o WebSocket por decidir).
- **Sandbox:** rootless Podman candidato; Bubblewrap para procesos más pequeños cuando convenga y resulte verificable.
- **Telemetría:** OpenTelemetry Go para trazas/métricas, logs estructurados, exportador local inicial, OTLP opcional.

Ninguna selección propuesta justifica desactivar seguridad o simular evidencia de éxito.

## 4. Límites de responsabilidad

```text
                 Factory UI / CLI
                        |
                    Factory Core
       +----------------+----------------+
       | Action/Campaign, políticas y estado |
       | Scheduler / Resource Governor       |
       | Agent Composer / Model Router       |
       | Knowledge / Skill Registry          |
       | Quality / Evidence / Delivery       |
       | Incidents / Maintenance / Telemetry |
       +----------------+----------------+
                        |
               Core–Worker protocol
                        |
           Worker local (remotos después)
                        |
           Sandbox de Run / workspace
                        |
             Paseo y agentes reales
```

El **Core** es autoridad para estado, políticas, presupuesto, identidad, autorización y efectos externos. El **Worker** materializa una asignación firmemente identificada, ejecuta y devuelve eventos/resultados; no concede permisos ni fusiona PR por decisión propia. **Paseo** gobierna sesiones de agentes, no el estado normativo de Actions. La **UI** muestra proyecciones y envía intenciones autorizadas al Core; nunca modifica directamente la base ni controla workers por una ruta paralela.

Las responsabilidades son módulos de dominio, **no microservicios**. El Scheduler es determinista, no otro LLM.

## 5. Entidades persistentes y flujo

- **Project:** identidad, repositorios autorizados, contexto y Quality Profile.
- **Incident:** ocurrencia o grupo deduplicado de fallos reales y su evidencia.
- **Action:** intención de ingeniería, objetivo, riesgo, spec, criterios de aceptación y estado.
- **ActionSpec:** requisitos versionados; la revisión fijada gobierna cada ejecución.
- **Campaign:** conjunto de Actions y dependencias bajo un objetivo y presupuesto.
- **Task:** unidad de trabajo asignable.
- **Run:** intento de ejecución con configuración, recursos, workspace, estado y resultado.
- **AgentSession:** participante real, rol, runtime/modelo, sesión Paseo y permisos efectivos.
- **CandidateRevision:** contenido exacto del cambio propuesto, vinculado a commit/hash.
- **EvidenceBundle:** pruebas y revisiones sobre el candidato exacto.
- **Delivery:** intención y recibos de PR/merge, con reconciliación de efectos.
- **PolicyRevision / SkillVersion / RouteRevision:** versiones que permiten auditar y repetir decisiones.

Ruta principal: **intake → deduplicación/triage → spec proporcional al riesgo → composición → presupuesto/autoridad → Run aislado → candidato → pruebas y revisión → Acceptance Gate → delivery autorizado → finalización**.

No confundir `Run completed` con `Action accepted`, `PR merged` ni `Core deployed`.

## 6. Agentes y evaluación semántica

Roles disponibles: Director, Planner/Architect, Explorer, Librarian, Spec Manager, Implementer, Reviewer, Repairer e Incident Analyst. Son **responsabilidades activables**, no nueve sesiones permanentes. El Agent Composer selecciona el equipo mínimo, modelos, skills, herramientas, permisos, dependencias y presupuesto.

Semantic Assessment es una **capacidad sin autoridad ejecutiva**, para intent, complexity, stack, knowledge gap, failure diagnosis, impact, validation need y spec ambiguity. En cada evaluación: evidencia acotada y vinculada, salida tipada, incertidumbre explícita, validador determinista, presupuesto y vigencia. No usar un modelo para hechos que el Core puede verificar directamente.

Model Router: perfiles ECONOMY/BALANCED/REASONING/CRITICAL_REVIEW; elegibilidad primero, optimización después. Las combinaciones concretas runtime/proveedor/modelo/configuración deben superar pruebas reales de herramientas y salida estructurada. Fallback visible y compatible; bloqueos explícitos. Las rutas promovidas solo afectan nuevas asignaciones autorizadas; los Runs existentes mantienen su configuración fijada.

## 7. Knowledge Resolver y Skill Registry

El Knowledge Resolver distingue conocimiento suficiente, recuperable, faltante, obsoleto, contradictorio y no verificado. El Librarian investiga cuando existe una brecha real; ausencia de skill no implica automáticamente incapacidad.

Skill Registry: identificador, versión, hash, origen, licencia, dependencias, competencias, compatibilidad, pruebas, riesgos y ámbito GLOBAL/PROJECT/RUN. Las skills externas pueden descubrirse, evaluarse y ejecutarse automáticamente *solo* dentro de un sandbox y las capacidades ya autorizadas. La admisión no otorga privilegios. Scripts, dependencias y contenido no confiable se evalúan en cuarentena; no se ejecutan en el Core. La promoción GLOBAL requiere evaluación independiente de seguridad, compatibilidad y utilidad; existe revocación y rollback. Una skill no puede modificar su propio evaluador o políticas de autorización.

## 8. Ejecución, aislamiento y seguridad

- Sandbox por Run es **propuesta preferida**, aún pendiente de comprobar técnicamente con Paseo.
- Cada Run obtiene snapshot/checkout autocontenido con commit fijado y workspace; Git worktree **no** cuenta como aislamiento OS.
- No montar HOME del host, sockets privilegiados, tokens de GitHub, base de datos del Core ni carpetas de otros proyectos.
- Los reviewers no tendrán escritura sobre el candidato revisado; su independencia se comprueba por permisos, identidad y evidencia.
- Core verifica efectos externos en el momento de ejecutarlos. Tokens de entrega quedan fuera del agente.
- Workers remotos se conectarán mediante identidad propia, autenticación mutua, leases, heartbeats, fencing y reconciliación, sin escrituras directas a SQLite.
- **Bloqueador crítico:** ejecutar Paseo y un shell no confiable en el mismo sandbox puede exponer tokens de modelos. Debe demostrarse separación efectiva de credenciales y herramientas o mitigación suficiente. Si no se logra, F1 no pasa; prohibido fallback no aislado.

## 9. Quality Baseline y Delivery

Baseline global innegociable: identidad candidato/spec/política; sandbox verificado; alcance; aceptación funcional; ausencia de regresiones relevantes; seguridad; revisión independiente exigida; procedencia de evidencia; coincidencia entre candidato verificado y commit entregado. Project Quality Profile agrega pruebas pertinentes, nunca reduce garantías globales.

Estados de control: ACCEPTED, REPAIR_REQUIRED, BLOCKED, HUMAN_REQUIRED, NOT_TESTED. No equiparar SKIPPED, NOT_APPLICABLE o NOT_TESTED con PASS.

Riesgos: R0 bajo, R1 moderado, R2 alto, R3 crítico. **Merge autónomo solo si riesgo final R0**, todas las verificaciones y protecciones GitHub satisfechas sobre el commit exacto, sin cambio de rama/política/candidato desde la aceptación. Implementer no recibe permiso de merge. La promoción/despliegue del Core es otra operación posterior.

## 10. Autodesarrollo y mantenimiento permanente

La Factory es proyecto consumidor propio. El Core estable **A** gobierna una campaña que modifica el candidato **B**, en entorno aislado. B no controla A, sus reglas, presupuesto ni credenciales. PR/merge de B no equivale a despliegue B. Update Supervisor externo/estable se diseña ahora y se implementa más adelante.

La monitorización de incidentes permanece activa sin sesiones LLM constantes. Incidentes se agrupan por huella; se generan/actualizan Actions y se inician reparaciones autorizadas bajo límites de riesgo, presupuesto y progreso. Los incidentes que afectan integridad, autorización, sandbox o base requieren contención antes de reparación; no se autorreparan mediante una vía R0 normal.

## 11. Resource Governor

Reservas para **Development, Maintenance y Evaluation**, con préstamo de capacidad ociosa, recuperación en puntos seguros, prioridad a incidentes críticos y equidad multiproyecto. Presupuestos jerárquicos Factory → Project → Campaign → Action → Run/Agent, más reserva de mantenimiento. Dimensiones: turnos/tokens observados, tiempo, concurrencia, CPU/RAM/disco, reintentos y gasto cuando sea efectivamente medible. Datos desconocidos ≠ cero. Los agentes pueden redistribuir asignaciones subordinadas dentro de límites; no ampliar el techo autorizado ni evadirlo creando campañas hijas.

## 12. Evaluaciones y aprendizaje

Corpus de tareas reproducibles con commit/fixture congelados y oráculo independiente; registrar modelo, runtime, skill, ruta, prompts/config, herramientas, resultados, latencia, variabilidad, first-pass, reparaciones y costo total por aceptación. Primero cumplir umbrales de seguridad/calidad, después comparar eficiencia. Champion/Challenger con promoción controlada únicamente para rutas elegibles de bajo riesgo, nunca mediante autoevaluación del candidato ni por una victoria aislada. Las evaluaciones consumen presupuesto propio y las decisiones se versionan.

## 13. Factory Office

F1 exige UI propia mínima: Dashboard, Actions, Agents y Action Detail. Debe permitir crear/consultar Actions, observar sesiones Paseo, revisar Timeline/Quality/Delivery y gestionar controles según permisos. Estado desde Core y stream de eventos duraderos; recuperar por snapshot + replay al reconectar, sin datos fabricados. Una futura oficina pixel-art será otra vista de los mismos datos.

## 14. Observabilidad desde el día cero

Distinguir: (1) historial de eventos de negocio, transaccional y no muestreable; (2) spans/métricas OpenTelemetry operacionales; (3) logs estructurados y artefactos de diagnóstico. El backend OTLP es opcional: Core y Worker deben poder operar sin colector. Propagar `traceId` entre Core, Worker, adaptador Paseo, validaciones y delivery; incluir `projectId`, `actionId`, `runId`, `agentId`, `sessionId` cuando aplique. No registrar secretos, prompts completos o cuerpo de repositorios como atributos. **Véase `docs/observability/telemetry.md`.**

## 15. F0 y F1

**F0 (documentación primero):** aprobar contratos Project/Action, Campaign/Run, Core/Worker, AgentSession, Quality/Delivery, FactoryEvents; pruebas de viabilidad y riesgos de sandbox/credenciales; baseline; telemetría; matriz de aceptación. Documentación y ADR en Git como primer commit, cuando se autorice crear el repositorio.

**F1 (flujo vertical):** (1) Core persistente, API, eventos/telemetría y UI mínima; (2) Worker y reconciliación; (3) Paseo + sandbox viable; (4) implementación + revisión independientes + Quality Baseline; (5) PR/merge R0 condicionado; (6) campaña real sobre el propio repositorio. Un adaptador único, un Worker local y un primer proyecto real; otro repositorio fixture verifica aislamiento multiproyecto.

F1 **no** incluye workers remotos reales, promoción automática de versiones, búsqueda global avanzada de skills, routing adaptativo completo ni oficina animada.

## 16. Reglas de validación de incrementos

Todo incremento: build/tests aplicables → el Implementer prueba y corrige → candidato fijado → Independent Verifier ejecuta recorridos negros CLI/API/navegador/agent-runtime sobre checkout limpio → pruebas negativas y reinicio/recuperación cuando aplique → evidencias guardadas → Reviewer independiente → aceptación determinista. El implementador reproduce fallos y repara dentro de presupuesto; no puede autodeclarar PASS, modificar el oráculo ni omitir las pruebas. Tras cambios, revalidar evidencia afectada; los tests no sustituyen el recorrido real.

La evidencia de F1 incluye: Run real de Paseo que modifica el repositorio; sandbox probado; restart y deduplicación; calidad sobre commit exacto; Reviewer separado; PR real; merge R0 real elegible y rechazo real de uno inelegible; UI mostrando sesiones/eventos reales; entrega de una mejora funcional del producto usando Core A sobre candidato B.

## 17. Cambios arquitectónicos y fuente de verdad

- `AGENTS.md` es el punto de entrada operativo para agentes; **no** reemplaza esta especificación.
- Este `MASTER.md` define el mapa del producto y los invariantes.
- `docs/contracts/` profundiza interfaces comprobables; `docs/architecture/decisions/` conserva el porqué.
- Cambio significativo: Action + ADR propuesto → autoridad/riesgo → revisión → actualización simultánea de MASTER/contratos/tests → PR → aceptación.
- Un ADR aprobado se sustituye mediante nuevo ADR, no borrando decisiones previas. Cambios de seguridad, política de merge, aislamiento o aprobación son protegidos y no R0.
- El estado real de implementación y los resultados se declaran con pruebas, no con la existencia de documentación.

## 18. Bloqueadores y decisiones abiertas

| ID | Bloqueador/decisión | Prueba requerida |
|---|---|---|
| SEC-01 | Separación Paseo/ejecución no confiable/credenciales | Prueba real de aislamiento, incluyendo intentos de lectura de secretos y acceso cross-project |
| EXEC-01 | Sandbox por Run y ciclo de vida Paseo | Launch/inspect/close/recover, con registro de IDs y consumo de recursos |
| TECH-01 | Go + adaptador TS | Prototipo limitado de integración con funciones reales del SDK |
| DATA-01 | SQLite WAL y reconciliación | Crash, leases, idempotencia, transacciones y replay |
| UI-01 | Transporte de eventos a navegador | Reconexión, autorización y eventos perdidos/duplicados |
| POLICY-01 | Umbrales de calidad y presupuestos iniciales | Calibración empírica y pruebas de política |
| NAME-01 | Nombre definitivo del producto/repositorio | Decisión explícita antes de crearlo |

## 19. Referencias

- AEH anterior (para extracción selectiva de conocimientos): https://github.com/JamesMorales04/agentic-engineering-harness
- AGENTS.md: https://agents.md/
- ADR/MADR: https://adr.github.io/ y https://github.com/adr/madr
- OpenTelemetry Go: https://opentelemetry.io/docs/languages/go/
- Playwright: https://playwright.dev/docs/trace-viewer

**Este documento es borrador de diseño y no prueba funcionalidad ni autoriza por sí solo despliegues, merges o nuevos privilegios.**

## Decisiones de diseño pendientes de ratificación

- Go frente a Rust es una propuesta, no una elección definitiva: ver `docs/architecture/decisions/ADR-0005-go-vs-rust-pending.md`.
- SEC-01 Paseo/sandbox/credenciales no está certificado: ver `docs/security/paseo-sandbox-spike.md`.
- La aceptación por recorrido de usuario pertenece al Independent Verifier durante Review: ver ADR-0004.
