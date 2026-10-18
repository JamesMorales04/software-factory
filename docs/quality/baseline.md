# Quality Baseline global obligatorio

> **Decisión:** acordada. **Estado de implementación:** no iniciado.

## Invariantes de aceptación

Todo cambio debe demostrar, mediante pruebas apropiadas a su naturaleza: identidad del proyecto, Action, spec y candidato; ejecución dentro de límites autorizados; alcance respetado; criterio funcional verificable; regresión pertinente; seguridad; revisión independiente cuando la política la exige; procedencia de evidencias; y correspondencia exacta entre lo revisado y lo entregado.

Project Quality Profile **añade** verificaciones según lenguaje, framework, dependencias, arquitectura y riesgo. No puede desactivar el baseline ni falsificar `NOT_APPLICABLE`. Calidad universal no significa que todos los proyectos ejecuten todas las herramientas.

## Estados

`ACCEPTED`: controles obligatorios satisfechos con evidencia; `REPAIR_REQUIRED`: defectos comprobados y reparables; `BLOCKED`: falta capacidad, entorno o evidencia; `HUMAN_REQUIRED`: frontera de autoridad/resolución de producto; `NOT_TESTED`: sin prueba suficiente. `PASS`, `FAIL`, `NOT_APPLICABLE` y `SKIPPED` se conservan por chequeo; `SKIPPED` y `NOT_TESTED` no equivalen a PASS.

## Riesgo y merge

- **R0:** cambio final acotado, bajo riesgo, elegible para merge autónomo **solo** tras verificación, revisión y CI requeridos.
- **R1:** modificación ordinaria/moderada; control adicional según política.
- **R2:** seguridad, migraciones o interfaces públicas; vía protegida.
- **R3:** autoridad, credenciales, sandbox, policy/Quality Gate, merge o actualización del Core; vía protegida estricta.

El riesgo **final** se recalcula sobre el diff real. No aceptar una clasificación inicial R0 como autorización definitiva. Comprobar candidate SHA/commit, política, estado de CI y reglas GitHub inmediatamente antes del efecto; revalidar si cambian. Tokens de merge solo en Delivery Engine, fuera de sesiones agentic.

## Verificación como usuario real

Por incremento: unit/contract/integration según alcance + ejecución externa del artefacto real + prueba del trayecto usuario (CLI, HTTP, Playwright, Paseo real) + pruebas negativas + recopilación de evidencia + Reviewer independiente. Si falla, el Implementer repara dentro del presupuesto y se repiten pruebas afectadas. El implementador no tiene autoridad de aceptación ni puede modificar el oráculo de forma oportunista.

## Pruebas de base F1

- Fallo real del test obligatorio bloquea el merge.
- Reporte de Reviewer independiente queda vinculado a candidato/política.
- CI pendiente o SHA divergente bloquea la entrega.
- Acción que toca archivos R2/R3 no usa ruta R0 aunque comenzara como trivial.
- El recorrido de navegador/CLI encuentra y corrige al menos un defecto reproducible durante el desarrollo del producto.
