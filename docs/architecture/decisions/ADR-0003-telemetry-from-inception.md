---
id: ADR-0003
status: Proposed
date: 2026-10-18
owner: product-owner
---
# ADR-0003 — Telemetría desde el primer incremento funcional

## Contexto
La Factory necesita diagnosticar fallos, recuperar Runs, medir recursos, observar agentes y comparar modelos. Añadir telemetría después haría invisibles los primeros problemas de integración.

## Alternativas
1. Integrarla al final de F1: menor trabajo inicial, poca trazabilidad de fallos tempranos.
2. Instrumentación mínima desde el primer Core con persistencia, evolutiva: costo inicial moderado y evidencia temprana.

## Decisión propuesta
Alternativa 2: eventos duraderos transaccionales para estado; logs estructurados con redacción de secretos; trazas y métricas OTel, export local inicial y OTLP opcional. Core opera aunque el exportador esté caído.

## Consecuencias
Necesidad de IDs y convenciones compartidas desde F1. Prohibido convertir telemetría mutable en autoridad o incluir secretos/prompts/código en atributos.

## Verificación
Una Action creada, consultada y recuperada tras reinicio debe contar con eventos y trazas correlacionables; apagar el exportador no altera la transición; los logs no filtran secretos de pruebas.
