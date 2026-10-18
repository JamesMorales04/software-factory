# ADR-0005 — Selección de lenguaje del Core (Go frente a Rust)

- **Estado:** Propuesto / PENDIENTE DE RATIFICACIÓN.
- **Contexto:** Core/Worker coordinan APIs, concurrencia, estado y procesos; Paseo SDK oficial está en TypeScript. El riesgo crítico es aislamiento del runtime y credenciales, no gestión de memoria dentro del Core.
- **Alternativas:** Go (ergonomía concurrencia, API y menor tiempo esperado de integración) y Rust (ownership, control de memoria y garantías contra errores de memoria en compilación a cambio de mayor complejidad inicial).
- **Propuesta:** Go para Core/Worker; TypeScript para adapter Paseo. No suponer que Go es intrínsecamente seguro para sandbox ni que Rust resuelve exposición de secretos por un proceso externo.
- **Prueba previa:** comparar una integración vertical pequeña: API + job durable + subprocess + persistencia y recuperación; tiempo/costo real, complejidad, latencia/CPU/RAM, calidad. Decidir antes del primer commit de implementación del Core.
- **Consecuencias:** Si se ratifica Go, documentar límites de concurrencia y race detection. Si gana Rust, adaptar stacks sin cambiar contratos de alto nivel.
