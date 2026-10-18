# Software Factory — Arquitectura F0 (borrador)

Este repositorio público almacena la **especificación arquitectónica en revisión** de Software Factory: una plataforma local-first y multiproyecto orientada al desarrollo autónomo, con verificación independiente, ejecución aislada y trazabilidad de cada entrega.

Actualmente contiene documentación. **No certifica funcionalidades implementadas** ni dispone todavía de una versión operativa de Factory Core, Workers o Factory Office.

## Documentación principal

- [AGENTS.md](AGENTS.md): instrucciones iniciales y límites de autoridad para agentes implementadores.
- [Especificación maestra](docs/architecture/MASTER.md): arquitectura, decisiones y restricciones.
- [Índice documental](docs/README.md): organización, contratos y cambios mediante ADR.
- [Plan F0/F1](docs/implementation/f0-f1-plan.md): hitos, validaciones y definición de terminado.
- [Prueba SEC-01](docs/security/paseo-sandbox-spike.md): viabilidad de Paseo, sandbox y aislamiento de credenciales.
- [Quality Baseline](docs/quality/baseline.md): garantías de aceptación obligatorias.
- [Observabilidad](docs/observability/telemetry.md): eventos, trazas y métricas desde el inicio.
- [Prompt de campaña](docs/implementation/campaign-prompt.md): orientación para el futuro Campaign Director.

**Estado:** F0 en preparación; selección Go/Rust y aislamiento Paseo pendientes de comprobación. Los documentos distinguen decisiones acordadas, propuestas e implementación no verificada.
