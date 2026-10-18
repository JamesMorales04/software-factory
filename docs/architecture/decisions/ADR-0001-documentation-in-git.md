---
id: ADR-0001
status: Proposed
date: 2026-10-18
owner: product-owner
---
# ADR-0001 — Arquitectura y decisiones versionadas en Git

## Contexto
La experiencia de AEH evidenció el costo de decisiones duplicadas, documentación divergente y afirmaciones de capacidades no demostradas. Los agentes necesitan un punto de entrada breve y una fuente de verdad verificable.

## Alternativas
1. Conversaciones/wiki como autoridad principal: difícil de vincular a commits y PR.
2. Documentación versionada en el mismo repositorio: permite revisión, trazabilidad y pruebas de enlaces.

## Decisión propuesta
Mantener `docs/architecture/MASTER.md`, ADRs y contratos bajo control de versiones; `AGENTS.md` en raíz como índice operativo. Toda modificación arquitectónica relevante exige sincronización documental y revisión.

## Consecuencias y límites
Fácil auditoría y onboarding de agentes; costo de mantener referencias. La documentación no sustituye políticas del Core ni evidencia de ejecución.

## Verificación
Cada PR relevante identifica la Action y el ADR si aplica, enlaces válidos y cambios documentales sincronizados. Ninguna capacidad se etiqueta `VERIFIED` sin recorrido real de aceptación.
