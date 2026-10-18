---
id: ADR-0002
status: Accepted
date: 2026-10-18
owner: product-owner
---
# ADR-0002 — Quality Baseline global obligatorio

## Contexto
Una Factory multiproyecto debe estandarizar garantías sin imponer exactamente las mismas herramientas a todos los lenguajes/frameworks.

## Alternativas
1. Un conjunto fijo de herramientas para todos: gasto y fallos espurios.
2. Solo perfiles por proyecto: posibilidad de debilitar requisitos comunes.
3. Garantías globales con verificaciones pertinentes y perfiles aditivos por proyecto.

## Decisión
Adoptar la alternativa 3: identidad, aislamiento, alcance, corrección, regresión, seguridad, revisión, evidencia y entrega verificables son requisitos globales; el perfil añade verificaciones propias. Ningún proyecto puede debilitarlos silenciosamente.

## Consecuencias
La ejecución de herramientas depende del riesgo y tipo de cambio. Un `NOT_TESTED` o `SKIPPED` no es PASS. Política de entrega y riesgo son autoridad del Core, no de los agentes.

## Verificación
Demostrar en F1 que una prueba obligatoria ausente o fallida impide aceptación y que las validaciones guardan el SHA exacto del candidato.
