# Observabilidad desde el primer incremento

> **Estado:** propuesta técnica F0. Se implementará desde el primer Core funcional; pendiente de aceptación y pruebas reales.

## 1. Tres planos independientes

| Plano | Fuente de verdad | Garantía |
|---|---|---|
| Estado y eventos de negocio | SQLite Core; cambios de estado y evento dentro de la misma transacción | No muestreable, durable, idempotente, replay por secuencia |
| Trazas y métricas | OpenTelemetry, exportador local inicialmente y OTLP opcional | Diagnóstico/optimización; puede ser muestreado o fallar sin bloquear negocio |
| Logs/artefactos | Logs estructurados locales y artefactos con referencias/digests | Diagnóstico acotado, política de acceso y retención |

**No utilizar una traza OTel como recibo de entrega**, ni un log como fuente de estado. OTel collector y plataformas externas son opcionales.

## 2. Identidad y eventos

Atributos de correlación según procedencia: `traceId`, `spanId`, `projectId`, `actionId`, `campaignId`, `taskId`, `runId`, `attempt`, `workerId`, `agentId`, `paseoSessionId`, `candidateDigest`, `policyRevision`. No todos son aplicables a todos los eventos; campos no disponibles quedan nulos/desconocidos, no fabricados.

Esquema conceptual `FactoryEvent`:

- `eventId`, `eventType`, `schemaVersion`, `aggregateType`, `aggregateId`, `sequence`.
- `occurredAt` (origen, puede ser no fiable) y `recordedAt` (Core).
- `origin`, `actor`, `correlation`, `causationId`, `payload` tipado y redactado.
- `evidenceRefs` y `payloadDigest` para referencias grandes y auditoría.

Persistir secuencias monotónicas por agregado y detectar replay/reenvíos mediante identidades. No prometer orden temporal global entre Workers sin un mecanismo adicional. Estados autoritativos y eventos relacionados se confirman atómicamente cuando se emiten desde Core; eventos remotos requieren reconciliación y deduplicación.

## 3. Eventos iniciales

`CoreStarted`, `CoreStopped`, `WorkerRegistered`, `WorkerHeartbeat`, `WorkerDisconnected`, `ActionCreated`, `ActionTransitioned`, `RunQueued`, `RunLeased`, `RunStarted`, `RunProgressObserved`, `RunCompleted`, `RunFailed`, `PaseoSessionBound`, `AgentTurnStarted`, `AgentTurnFinished`, `ToolCallObserved`, `ValidationStarted`, `ValidationFinished`, `CandidateRecorded`, `ReviewRecorded`, `DeliveryIntentCreated`, `DeliveryReceiptReconciled`, `PolicyDenied`, `IncidentObserved`.

No implementar todos desde el primer commit: instrumentar los que correspondan a cada vertical slice y comprobarlos desde ese incremento. Un tipo nuevo requiere documentación de versión, prueba de serialización, replay y política de redacción.

## 4. Trazas OpenTelemetry

Instrumentar HTTP y operaciones Core/Worker mediante spans padre-hijo, propagando contexto entre procesos. Spans sugeridos: `factory.action.create`, `factory.run.schedule`, `factory.worker.execute`, `factory.paseo.launch`, `factory.agent.turn`, `factory.validation.run`, `factory.delivery.check`, `factory.delivery.merge`. No declarar que un span prueba que un efecto ocurrió; asociar recibos persistidos del Core.

Go OTel: métricas y trazas; para logs preferir inicialmente logging estructurado, correlacionado con trace/span, sin bloquear por madurez cambiante de SDK. Una falla de exportación no bloquea ni valida efectos. Limitar cardinalidad: IDs únicos pueden ir en trazas/logs, **no** como etiquetas de métricas agregadas.

## 5. Métricas iniciales

- Acciones iniciadas/completadas/bloqueadas por categoría/resultado.
- Runs activos, cola, duraciones y fallos por etapa.
- Tiempo de espera de Worker y latencia de reinicio/reconciliación.
- Turnos, tokens y costo **cuando los informa un proveedor** (con `known/partial/unknown`).
- Reintentos, reparaciones, first-pass y ejecución de gates.
- CPU/RAM de Worker y sandbox cuando estén disponibles.
- Fracción de eventos de sesión observados y pérdida de cobertura.

Medir desde el primer incremento solo lo que efectivamente existe; ampliar sin fabricar ceros.

## 6. Seguridad y retención

- No telemetría con secretos, OAuth, tokens, headers de autorización, prompts completos, outputs íntegros ni contenido de repositorios por defecto.
- Almacenar referencias a artefactos grandes y hashes; acceso por proyecto/rol. Redacción antes de exportar.
- Logs con rotación/límites de tamaño, métricas de cardinalidad controlada, retención configurable; no fijar valores ficticios antes de pruebas.
- Export local sin necesidad de cuenta externa; OTLP futuro configurable.
- Pruebas negativas con secretos sintéticos deben acreditar que los exportadores no filtran datos.

## 7. Acceptance tests observabilidad F1

1. Crear Action desde UI/API, cerrar navegador y reiniciar Core: estado e historial persisten.
2. Transición confirmada y evento coincidente; reenvío no duplica efectos.
3. Recuperar estado desde snapshot + eventos posteriores al reconectar UI.
4. Atribuir Run/Worker/Paseo a la misma cadena causal mediante IDs.
5. Deshabilitar exportador OTel: el Core sigue funcionando y guardando eventos.
6. Redacción de secretos y control de acceso a artefactos.
7. Faltan tokens reportados: estado `UNKNOWN/PARTIAL`, nunca cero estimado presentado como real.
