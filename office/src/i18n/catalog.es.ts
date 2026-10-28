import type { Catalog } from "./catalog.en";

export const es: Catalog = {
  "common.retry": "Reintentar",
  "common.errorTitle": "Algo salió mal",
  "common.unknown": "Desconocido",
  "common.notRecorded": "sin registrar",
  "common.notSet": "sin definir",
  "common.dash": "—",
  "common.actions": "Acciones",

  "nav.group.overview": "Resumen",
  "nav.group.work": "Trabajo",
  "nav.group.operations": "Operaciones",
  "nav.overview": "Resumen",
  "nav.decisionInbox": "Bandeja de decisiones",
  "nav.projects": "Proyectos",
  "nav.actions": "Acciones",
  "nav.lead": "Lead",
  "nav.delegations": "Delegaciones",
  "nav.campaigns": "Campañas",
  "nav.agentOffice": "Oficina de agentes",
  "nav.runs": "Ejecuciones",
  "nav.workers": "Trabajadores",
  "nav.runtime": "Entorno de ejecución",
  "nav.settings": "Configuración",
  "nav.campaignDetail": "Detalle de la campaña",
  "nav.actionDetail": "Detalle de la acción",
  "nav.agentParticipant": "Participante de la oficina de agentes",
  "nav.factoryOffice": "Factory Office",

  "settings.language.title": "Idioma",
  "settings.language.description":
    "Elige el idioma de los controles, etiquetas y mensajes de Office. Los identificadores de Core, los códigos de estado, las rutas de proveedores y el contenido de los repositorios nunca se traducen.",
  "settings.language.label": "Idioma de la interfaz",
  "settings.language.note":
    "La preferencia se guarda en este dispositivo y se aplica de inmediato.",

  "access.loading": "Cargando acceso y sesiones de dispositivos…",
  "access.error.load": "El estado de acceso no está disponible.",
  "access.error.unknownTitle": "Estado de acceso desconocido",
  "access.error.unknownDescription": "Core no devolvió información de acceso.",
  "access.local.title": "Acceso local",
  "access.local.description": "Estado de acceso de Office según Core",
  "access.local.refresh": "Actualizar estado",
  "access.local.sessionNote":
    "Office usa una sesión de navegador independiente y revocable. La credencial de recuperación del operador no se guarda en este navegador.",
  "access.remote.title": "Acceso remoto privado",
  "access.remote.connected":
    "Core puede alcanzar el endpoint privado de salud de Office. Esto solo confirma la ruta de Serve actual; Factory sigue exigiendo su propia sesión de dispositivo inscrita.",
  "access.remote.disconnected":
    "Core no puede alcanzar el endpoint privado de salud de Office configurado. Revisa el servicio de Tailscale, la configuración de Serve y el endpoint HTTPS privado en `docs/operations/compose-deployment.md`.",
  "access.remote.unknown":
    "No se pudo verificar la conectividad con Tailscale. Revisa el servicio y el endpoint HTTPS privado en `docs/operations/compose-deployment.md`. No trates la pertenencia a la tailnet como una sesión de Factory.",
  "access.remote.disabled":
    "El acceso remoto está deshabilitado. Para habilitarlo, sigue la configuración opcional del overlay de Tailscale en `docs/operations/compose-deployment.md`.",
  "access.remote.endpoint": "Endpoint HTTPS privado:",
  "access.remote.noEndpoint":
    "Actualmente no hay ningún endpoint HTTPS privado configurado.",
  "access.remote.localAvailable":
    "La conexión local de Office sigue disponible. El overlay de Tailscale no publica los servicios de Core, Worker ni runtime.",
  "access.remote.guidanceTitle":
    "Configuración HTTPS de Tailscale y privacidad",
  "access.remote.guidance":
    "Habilita MagicDNS y los certificados HTTPS en la configuración de tu tailnet. Los nombres de host de los certificados de Tailscale se publican en los registros públicos de Certificate Transparency, por lo que el nombre de host del dispositivo puede ser visible públicamente. El certificado no publica Factory: Office permanece privado dentro de la tailnet y Funnel sigue deshabilitado.",
  "access.devices.title": "Dispositivos inscritos",
  "access.devices.description":
    "Cada navegador y teléfono recibe su propia sesión.",
  "access.devices.pair": "Emparejar dispositivo",
  "access.devices.preparing": "Preparando…",
  "access.devices.revokeAll": "Revocar todos los dispositivos",
  "access.devices.revokeAllConfirm":
    "¿Revocar todas las sesiones de dispositivos inscritos, incluido este dispositivo? Tendrás que inscribirlo de nuevo.",
  "access.devices.emptyTitle": "No hay otros dispositivos inscritos",
  "access.devices.emptyDescription":
    "Empareja un teléfono u otro navegador para crear una sesión independiente.",
  "access.devices.refreshNote":
    "La lista de dispositivos se actualiza cada cinco segundos. La última actividad y la caducidad provienen de Core.",
  "access.pairing.scanTitle": "Escanea con la cámara del teléfono",
  "access.pairing.scanDescription":
    "Confirma que este código coincide en ambas pantallas antes de aprobar el dispositivo pendiente.",
  "access.pairing.verificationCode": "Código de verificación",
  "access.pairing.expires": "Caduca",
  "access.pairing.qrNote":
    "El QR solo contiene un identificador de desafío aleatorio. No concede acceso. Aquí se requiere aprobación antes de que Core cree una sesión para el teléfono.",
  "access.pairing.cancel": "Cancelar código de emparejamiento",
  "access.pairing.createdNotice":
    "Desafío creado. Caduca automáticamente y solo se puede usar una vez.",
  "access.pairing.cancelledNotice":
    "Código de emparejamiento cancelado. No se puede reclamar ni aprobar.",
  "access.pairing.createError":
    "No se pudo crear un desafío de emparejamiento.",
  "access.pairing.cancelError":
    "No se pudo cancelar el código de emparejamiento.",
  "access.pairing.revokeAllError":
    "No se pudieron revocar las sesiones de dispositivos.",
  "access.pairing.qrError":
    "No se pudo renderizar el código de emparejamiento.",
  "access.pairing.qrAria":
    "Código QR de emparejamiento de dispositivo de un solo uso",
  "access.audit.title": "Actividad de seguridad",
  "access.audit.description":
    "Eventos recientes de sesiones y emparejamiento registrados por Core.",
  "access.audit.emptyTitle": "Aún no hay eventos de seguridad",
  "access.audit.emptyDescription":
    "La creación, rotación y revocación de sesiones y las decisiones de emparejamiento aparecen aquí.",
  "access.audit.by": "por {actor}",
  "access.audit.unknownDevice": "Dispositivo desconocido",
  "access.audit.created": "Sesión creada",
  "access.audit.rotated": "Sesión rotada",
  "access.audit.revoked": "Sesión revocada",
  "access.audit.revokedAll": "Todas las sesiones revocadas",
  "access.audit.claimed": "Emparejamiento reclamado",
  "access.audit.approved": "Emparejamiento aprobado",
  "access.audit.rejected": "Emparejamiento rechazado",
  "access.audit.cancelled": "Emparejamiento cancelado",
  "access.audit.consumed": "Emparejamiento completado",
  "access.audit.revocationRequested": "Revocación solicitada",
  "access.device.verifyCodeBefore": "Verifica el código",
  "access.device.verifyCodeAfter": "en ambos dispositivos antes de aprobar.",
  "access.device.enterCode": "Introduce el código que muestra el teléfono",
  "access.device.confirmCodeAria":
    "Confirma el código de verificación que muestra el teléfono",
  "access.device.approvedWaiting":
    "Aprobado. Se espera que el teléfono complete el intercambio de un solo uso; la aprobación caduca si no se recoge.",
  "access.device.lastActivity": "Última actividad",
  "access.device.expires": "caduca el {date}",
  "access.device.approve": "Aprobar dispositivo",
  "access.device.reject": "Rechazar",
  "access.device.cancelApproval": "Cancelar aprobación",
  "access.device.signOut": "Cerrar sesión",
  "access.device.revoke": "Revocar",
  "access.device.signOutConfirm":
    "¿Cerrar la sesión de este dispositivo? Tendrás que inscribirlo de nuevo.",
  "access.device.revokeConfirm": "¿Revocar la sesión del dispositivo {name}?",
  "access.device.actionError": "Falló la acción sobre el dispositivo.",
  "access.device.unavailable": "No disponible",

  "pair.title": "Empareja este dispositivo",
  "pair.missingChallenge":
    "A este enlace de emparejamiento le falta su desafío de un solo uso. Pide al propietario que cree un nuevo código QR.",
  "pair.waitingOwner":
    "La aprobación está pendiente en el dispositivo del propietario inscrito. Verifica que el código coincida antes de aprobar.",
  "pair.verificationCode": "Código de verificación",
  "pair.waitingStatus":
    "Esperando la aprobación del propietario. Mantén esta página abierta; esta solicitud de un solo uso no se guarda en el almacenamiento del navegador.",
  "pair.approved": "Aprobación recibida. Abriendo Factory Office…",
  "pair.rejected":
    "El propietario rechazó esta solicitud de dispositivo. Pídele que cree un nuevo código de emparejamiento.",
  "pair.expired":
    "Esta solicitud de dispositivo caducó. Pide al propietario que cree un nuevo código de emparejamiento.",
  "pair.cancelled":
    "El propietario canceló esta solicitud de emparejamiento. Pídele que cree un nuevo código de emparejamiento.",
  "pair.confirmOnOwner":
    "Confirma esta solicitud en la Factory Office del propietario antes de que el teléfono reciba su propia sesión segura.",
  "pair.deviceName": "Nombre del dispositivo",
  "pair.deviceNamePlaceholder": "Mi teléfono",
  "pair.defaultDeviceName": "Dispositivo móvil",
  "pair.requesting": "Solicitando aprobación…",
  "pair.request": "Solicitar aprobación del propietario",
  "pair.errorStatus": "No se pudo comprobar el estado de la aprobación.",
  "pair.errorClaim":
    "Este desafío de emparejamiento no es válido o ha caducado.",
  "pair.privacyNote":
    "El identificador del desafío se elimina del historial del navegador antes de que Office contacte con Core. No concede acceso; se requiere la aprobación del propietario.",
  "pair.return": "Volver a Factory",

  "decisions.title": "Bandeja de decisiones",
  "decisions.description":
    "Trabajo respaldado por Core que requiere una decisión o conocimiento del operador. Nada de lo que aparece aquí se infiere de la prosa de un proveedor.",
  "decisions.actionableCount": "Requieren acción: {count}",
  "decisions.awarenessCount": "Para conocimiento: {count}",
  "decisions.asOf": "a fecha de {date}",
  "decisions.loading": "Cargando decisiones…",
  "decisions.error.load": "No se pudieron cargar las decisiones de Core.",
  "decisions.needsAction": "Requiere acción",
  "decisions.needsActionHint": "Solo se muestran operaciones y enlaces de Core",
  "decisions.emptyActionableTitle": "No hay nada que requiera acción",
  "decisions.emptyActionableDescription":
    "Las aprobaciones, las reparaciones de Quality Gate y el uso sin resolver aparecen cuando Core los registra.",
  "decisions.awareness": "Para conocimiento",
  "decisions.awarenessHint":
    "Solo monitorización; no se expone ninguna acción del operador",
  "decisions.emptyAwarenessTitle":
    "No hay trabajo en segundo plano sin resolver",
  "decisions.emptyAwarenessDescription":
    "Las sesiones de proveedor activas o huérfanas, las ejecuciones fallidas y las acciones que requieren intervención humana aparecen aquí.",
  "decisions.target.campaigns": "Abrir campañas",
  "decisions.target.lead": "Abrir Lead",
  "decisions.target.campaign": "Abrir campaña",
  "decisions.target.runtime": "Abrir el entorno de ejecución",
  "decisions.target.action": "Abrir acción",
  "decisions.target.default": "Abrir destino",
  "decisions.target.request": "Abrir solicitud",
  "decisions.subject": "Sujeto {id}",
  "decisions.refresh": "Actualizar",
  "decisions.live": "Proyección de Core en vivo",
  "decisions.stale": "Mostrando la última proyección cargada",
  "decisions.lastLoaded": "Última carga {date}",
  "decisions.offlineTitle": "Factory Core no está accesible",
  "decisions.offlineDetail":
    "Las acciones están deshabilitadas hasta que Core sea accesible. Los elementos mostrados son la última proyección cargada, no el estado en vivo.",
  "decisions.offlineActions":
    "Las acciones no están disponibles mientras Core no sea accesible.",
  "decisions.connectionError":
    "El navegador no pudo contactar con Factory Core.",
  "decisions.error.unauthorized":
    "Esta sesión de dispositivo no está autorizada para leer decisiones. Vuelve a inscribir o desbloquear este dispositivo.",
  "decisions.severity.critical": "Crítico",
  "decisions.severity.attention": "Atención",
  "decisions.severity.info": "Información",
  "decisions.context": "Contexto de la decisión",
  "decisions.project": "Proyecto",
  "decisions.campaign": "Campaña",
  "decisions.risk": "Riesgo",
  "decisions.change": "Cambio propuesto",
  "decisions.workUnits": "Unidades de trabajo",
  "decisions.acceptanceCriteria": "Comprobaciones de aceptación",
  "decisions.negativeCases": "Casos negativos",
  "decisions.paths": "Rutas",
  "decisions.impact": "Impacto en presupuesto y permisos",
  "decisions.budget": "Presupuesto",
  "decisions.permissions": "Permisos",
  "decisions.executionMode": "Modo de ejecución",
  "decisions.route": "Ruta de modelo",
  "decisions.tools": "Herramientas permitidas",
  "decisions.delegation": "Delegación",
  "decisions.noBudget": "No hay presupuesto registrado para esta decisión.",
  "decisions.noPermissions": "No hay permiso de ejecución registrado.",
  "decisions.actionUnavailable": "Bloqueado o no disponible:",
  "decisions.actionUnavailableDetail":
    "Core informa de una condición bloqueante de autoridad o evidencia. Sigue el espacio de trabajo enlazado; la Bandeja no la omitirá.",
  "decisions.evidence": "Evidencia e identificadores",
  "decisions.evidenceSummary": "Evidencia clave",
  "decisions.revision": "Revisión",
  "decisions.staleTitle": "El estado cambió",
  "decisions.staleBody":
    "El estado de Core cambió desde que se cargó este elemento. Revisa el elemento actualizado y vuelve a confirmar la acción.",
  "decisions.actions": "Acciones disponibles",
  "decisions.noActions":
    "No se expone ninguna mutación de la Bandeja para este elemento. Usa el espacio de trabajo enlazado para inspeccionarlo.",
  "decisions.measured": "medido",
  "decisions.unmeasured": "SIN MEDIR",
  "decisions.budget.attempts": "Intentos",
  "decisions.budget.sessions": "Sesiones",
  "decisions.budget.turns": "Turnos",
  "decisions.budget.inputTokens": "Tokens de entrada",
  "decisions.budget.outputTokens": "Tokens de salida",
  "decisions.budget.cost": "Techo de coste",
  "decisions.budget.deadline": "Fecha límite",
  "decisions.budget.wallClock": "Tiempo máximo",
  "decisions.budget.timeout": "Tiempo de espera",
  "decisions.budget.repairCycles": "Ciclos de reparación",
  "decisions.budget.parallel": "Unidades en paralelo",
  "decisions.budget.usage": "Uso",
  "decisions.budget.recordedCost": "Coste registrado",
  "decisions.action.approveIntake": "Aprobar plan congelado…",
  "decisions.action.approveIntakeSubmit": "Aprobar e iniciar campaña",
  "decisions.action.approved":
    "Core aprobó el plan congelado exacto e inició la campaña.",
  "decisions.action.approveIntakeConsequence":
    "Esto crea una campaña con el plan congelado, la revisión de perfil, la ruta y el presupuesto exactos. Core rechaza una identidad obsoleta; no se puede deshacer desde la Bandeja.",
  "decisions.action.rationale": "Motivo de la decisión",
  "decisions.action.rationaleHelp":
    "Core lo registra con la aprobación. No altera el plan congelado.",
  "decisions.action.confirmApprove":
    "He revisado el plan congelado, la revisión de perfil, el alcance, el presupuesto y los criterios de aceptación.",
  "decisions.action.requiredRationale": "Se requiere un motivo.",
  "decisions.action.cancelIntake": "Cancelar solicitud…",
  "decisions.action.cancelIntakeSubmit": "Cancelar solicitud",
  "decisions.action.intakeCancelled":
    "Core registró la cancelación de la solicitud.",
  "decisions.action.cancelIntakeConsequence":
    "Esto retira la solicitud de Lead asociada a la solicitud de intake. No cancela una campaña ya autorizada.",
  "decisions.action.reason": "Motivo",
  "decisions.action.reasonHelp": "Core lo registra con la decisión.",
  "decisions.action.confirmWithdraw":
    "Entiendo que esto retira la solicitud en el límite de Lead y no se puede deshacer aquí.",
  "decisions.action.requiredReason": "Se requiere un motivo.",
  "decisions.action.withdrawLead": "Retirar solicitud…",
  "decisions.action.withdrawLeadSubmit": "Retirar solicitud",
  "decisions.action.leadWithdrawn": "Core registró la retirada de Lead.",
  "decisions.action.withdrawLeadConsequence":
    "Esto registra una retirada autoritativa en el límite de Lead. Una campaña ya iniciada no se puede retirar aquí.",
  "decisions.action.cancelCampaign": "Cancelar campaña…",
  "decisions.action.cancelCampaignSubmit": "Cancelar campaña",
  "decisions.action.campaignCancelled":
    "Core registró la cancelación de la campaña.",
  "decisions.action.cancelCampaignConsequence":
    "Esto cancela la campaña y sus registros activos. La terminación del proveedor puede quedar sin confirmar; inspecciona la campaña después.",
  "decisions.action.confirmCampaignCancel":
    "Entiendo que la cancelación detiene la campaña; la terminación del proveedor puede quedar sin confirmar.",
  "decisions.action.completed": "Core registró la decisión.",
  "decisions.action.noConsequence":
    "Core registra esta operación. Revisa la proyección actualizada después.",
  "decisions.action.staleMissing":
    "Este elemento ya no está en la Bandeja. Puede que otro operador lo haya resuelto.",
  "decisions.action.alreadyStarted": "Ya se inició una campaña.",
  "decisions.action.requiredConfirmation":
    "Confirma la consecuencia antes de continuar.",
  "decisions.action.error": "La operación de Core falló.",
  "decisions.action.offline":
    "El navegador no pudo contactar con Factory Core. La acción no se envió.",
  "decisions.action.unauthorized":
    "Este dispositivo no está autorizado para esa operación de Core. Desbloquea o vuelve a inscribir el dispositivo.",
  "decisions.action.busy": "Aplicando…",
  "decisions.action.close": "Cerrar",
  "decisions.action.planHash": "Hash exacto del plan",
  "decisions.action.profileRevision": "Revisión exacta del perfil",
  "decisions.action.profileHash": "Hash exacto del perfil",
  "decisions.action.modelResolved": "Core liberó la reserva del modelo.",
  "decisions.action.unsupported":
    "Este tipo de acción de Core no está disponible en esta revisión de Office:",

  "projects.title": "Proyectos",
  "projects.description":
    "En el despliegue en contenedores, registra una URL Git HTTPS. Core la importa al almacenamiento gestionado por Factory y congela el commit base actual. El registro no concede autoridad de ejecución.",
  "projects.error.load": "No se pudieron cargar los proyectos.",
  "projects.error.register": "Falló el registro del proyecto.",
  "projects.error.configLoad":
    "No se pudo cargar la configuración del proyecto.",
  "projects.error.configSave":
    "Falló la actualización de la configuración del proyecto.",
  "projects.register.title": "Registrar un proyecto Git",
  "projects.register.label":
    "URL del repositorio Git HTTPS (o ruta gestionada)",
  "projects.register.submit": "Registrar proyecto",
  "projects.register.busy": "Registrando…",
  "projects.register.notice": "Se registró {id} en el commit base {commit}.",
  "projects.list.title": "Proyectos registrados ({count})",
  "projects.list.empty": "No hay proyectos registrados.",
  "projects.table.label": "Proyectos registrados",
  "projects.table.id": "ID",
  "projects.table.repository": "Repositorio",
  "projects.table.baseCommit": "Commit base",
  "projects.table.created": "Creado",
  "projects.table.actions": "Acciones",
  "projects.browseFiles": "Explorar archivos",
  "projects.configure": "Configurar",
  "projects.config.title": "Configuración del proyecto — {id}",
  "projects.config.revision": "Revisión {revision}",
  "projects.config.base": "base",
  "projects.config.hash": "hash",
  "projects.config.allowedPaths":
    "Rutas permitidas del repositorio (separadas por comas)",
  "projects.config.allowedPathsPlaceholder": "src, tests",
  "projects.config.capabilities": "Capacidades",
  "projects.config.qualityProfile": "Perfil de calidad",
  "projects.config.active": "Activo",
  "projects.config.save": "Guardar configuración del proyecto",
  "projects.config.saving": "Guardando…",
  "projects.config.notice":
    "Configuración del proyecto revisión {revision} guardada ({hash}).",

  "runs.title": "Ejecuciones",
  "runs.description":
    "Las ejecuciones se crean desde una campaña autorizada. Pon en cola un intento por cada ejecución en cola; el arrendamiento y la ejecución del Worker se observan aquí y la Office no los activa.",
  "runs.error.load": "No se pudieron cargar las ejecuciones.",
  "runs.error.queue": "Falló la puesta en cola del intento.",
  "runs.section.allRuns": "Todas las ejecuciones ({count})",
  "runs.list.empty":
    "Aún no hay ejecuciones. Autoriza una campaña y pon una ejecución en cola primero.",
  "runs.table.label": "Ejecuciones",
  "runs.table.run": "Ejecución",
  "runs.table.campaign": "Campaña",
  "runs.table.status": "Estado",
  "runs.table.attempts": "Intentos",
  "runs.table.created": "Creada",
  "runs.table.control": "Control",
  "runs.queue": "Poner intento en cola",
  "runs.queueing": "Poniendo en cola…",
  "runs.attemptQueued": "intento en cola",
  "runs.section.allAttempts": "Todos los intentos ({count})",
  "runs.attempts.empty": "No hay intentos en cola.",
  "runs.table.attempt": "Intento",
  "runs.notice.queued":
    "Intento {attempt} puesto en cola para la ejecución {run}.",

  "workers.title": "Trabajadores",
  "workers.description":
    "Solo credenciales de Worker registradas. Esta vista no inventa estado de actividad, latido ni ejecución.",
  "workers.error.load": "No se pudieron cargar los trabajadores.",
  "workers.section.registered": "Trabajadores registrados ({count})",
  "workers.list.empty": "No hay trabajadores registrados.",
  "workers.table.label": "Trabajadores registrados",
  "workers.table.worker": "Trabajador",
  "workers.table.credentialUpdated": "Credencial actualizada",

  "runtime.title": "Entorno de ejecución",
  "runtime.description":
    "Proyecciones duraderas de Core para sesiones de proveedor, reservas de llamadas a modelos, limpieza y telemetría. Un registro ausente no demuestra que la limpieza haya tenido éxito.",
  "runtime.meta.coreHealthy": "Core en buen estado",
  "runtime.meta.coreUnavailable": "Core no disponible",
  "runtime.meta.coreVersion": "Core {version}",
  "runtime.meta.fetched": "Obtenido {date}",
  "runtime.meta.workers": "Trabajadores registrados: {count}",
  "runtime.actions.inbox": "Bandeja de decisiones",
  "runtime.actions.settings": "Configuración",
  "runtime.error.load": "No se pudo cargar el estado del entorno de ejecución.",
  "runtime.staleSuffix":
    "La instantánea mostrada del entorno de ejecución puede estar desactualizada.",
  "runtime.loading": "Cargando el estado del entorno de ejecución…",
  "runtime.tabs.label": "Secciones del entorno de ejecución",
  "runtime.tabs.overview": "Resumen",
  "runtime.tabs.sessions": "Sesiones",
  "runtime.tabs.modelCalls": "Llamadas a modelos",
  "runtime.tabs.diagnostics": "Diagnóstico",
  "runtime.metrics.summaryAria": "Resumen del entorno de ejecución",
  "runtime.metrics.spend.label": "Gasto medido muestreado",
  "runtime.metrics.spend.detail":
    "Sesiones con uso conocido más las últimas 200 llamadas a modelos; no es un total (sesiones: {sessions}, llamadas: {calls})",
  "runtime.metrics.ceiling.label": "Techo reservado",
  "runtime.metrics.ceiling.detail":
    "Llamadas a modelos sin resolver: {calls} + turnos de proveedor abiertos: {turns}",
  "runtime.metrics.unresolved.label": "Llamadas sin resolver",
  "runtime.metrics.unresolved.detail": "Reservas de Core AMBIGUOUS o UNKNOWN",
  "runtime.metrics.openSessions.label": "Sesiones abiertas",
  "runtime.metrics.openSessions.detail":
    "Sesiones registradas: {total}; INTENT, ACTIVE, cancelación o huérfanas",
  "runtime.metrics.unmeasured.label": "Sesiones sin medición",
  "runtime.metrics.unmeasured.detail": "Excluidas del gasto medido",
  "runtime.metrics.telemetry.label": "Telemetría",
  "runtime.metrics.telemetry.detail": "cursor {cursor} · {pending} pendientes",
  "runtime.availability.title": "Disponibilidad de servicios",
  "runtime.availability.hint": "Solo se muestran hechos respaldados por Core",
  "runtime.availability.core": "Core",
  "runtime.availability.workers": "Trabajadores registrados",
  "runtime.availability.sessions": "Sesiones de proveedor",
  "runtime.availability.modelCalls": "Llamadas a modelos recientes",
  "runtime.availability.sampled": "Muestreadas: {count}",
  "runtime.availability.openTurns": "Turnos de proveedor abiertos",
  "runtime.availability.boundaryNote":
    "La API actual de Core no expone el estado del Supervisor ni del entorno de ejecución anidado. Office no infiere su salud a partir de un registro ausente, una vista desconectada o el estado de un proceso de Compose.",
  "runtime.states.title": "Estados de sesión",
  "runtime.states.inspect": "Inspeccionar sesiones",
  "runtime.states.emptyTitle": "No hay sesiones de proveedor",
  "runtime.states.emptyDescription":
    "Core no tiene registrada ninguna sesión de proveedor.",
  "runtime.workers.title": "Trabajadores registrados",
  "runtime.workers.hint":
    "El registro no es un latido ni un certificado de salud",
  "runtime.workers.emptyTitle": "No hay trabajadores registrados",
  "runtime.workers.emptyDescription":
    "Un Worker aparece después de registrarse; esto no demuestra que esté en línea.",
  "runtime.workers.table.label": "Trabajadores registrados",
  "runtime.workers.table.worker": "Trabajador",
  "runtime.workers.table.lastRegistration": "Último registro de credencial",
  "runtime.unresolved.title": "Reservas sin resolver",
  "runtime.unresolved.open": "Abrir llamadas a modelos",
  "runtime.unresolved.emptyTitle": "No hay llamadas a modelos sin resolver",
  "runtime.unresolved.emptyDescription":
    "No hay ninguna reserva de Core AMBIGUOUS o UNKNOWN registrada.",
  "runtime.unresolved.ceiling": "Techo {cost}",
  "runtime.sessions.title": "Sesiones de proveedor (mostradas: {count})",
  "runtime.sessions.hint":
    "Los recibos de limpieza son evidencia registrada, no prueba de aislamiento",
  "runtime.filters.state": "Estado",
  "runtime.filters.usage": "Uso",
  "runtime.filters.cleanup": "Limpieza",
  "runtime.filters.workerOrRoute": "Trabajador o ruta",
  "runtime.filters.workerOrRoutePlaceholder":
    "Filtrar por trabajador, proveedor o modelo",
  "runtime.filters.allStates": "Todos los estados",
  "runtime.filters.allUsage": "Todo el uso",
  "runtime.filters.measured": "Medido",
  "runtime.filters.unmeasured": "Sin medición",
  "runtime.filters.allCleanup": "Todos los estados de limpieza",
  "runtime.filters.receiptRecorded": "Recibo registrado",
  "runtime.filters.noReceipt": "Sin recibo",
  "runtime.sessions.emptyTitle": "No hay sesiones coincidentes",
  "runtime.sessions.emptyDescription":
    "Ajusta los filtros; no se infiere ningún estado para los registros ocultos.",
  "runtime.calls.title":
    "Llamadas a modelos propiedad de Core (mostradas: {count})",
  "runtime.calls.hint":
    "Las llamadas recientes se muestrean hasta 200; las llamadas sin resolver usan el libro completo",
  "runtime.filters.role": "Rol",
  "runtime.filters.allRoles": "Todos los roles",
  "runtime.filters.roleLead": "Lead",
  "runtime.filters.roleSemanticReviewer": "Revisor semántico",
  "runtime.filters.roleEvaluation": "Evaluación",
  "runtime.calls.emptyTitle": "No hay llamadas a modelos coincidentes",
  "runtime.calls.emptyDescription":
    "Ajusta los filtros; la muestra de llamadas recientes nunca oculta reservas sin resolver.",
  "runtime.calls.table.label": "Llamadas a modelos",
  "runtime.calls.table.reservation": "Reserva",
  "runtime.calls.table.roleScope": "Rol / ámbito",
  "runtime.calls.table.route": "Ruta",
  "runtime.calls.table.state": "Estado",
  "runtime.calls.table.usage": "Uso",
  "runtime.calls.table.ceiling": "Techo",
  "runtime.calls.table.updated": "Actualizada",
  "runtime.calls.scope": "ámbito",
  "runtime.calls.tokens": "{tokens} tokens / {cost}",
  "runtime.calls.unresolvedTitle": "Reservas sin resolver ({count})",
  "runtime.calls.unresolvedHint":
    "La liberación es explícita y auditada por Core; no es automática",
  "runtime.telemetry.title": "Exportación de telemetría",
  "runtime.telemetry.emptyTitle": "No hay estado de observabilidad",
  "runtime.telemetry.emptyDescription":
    "Core no devolvió el estado de telemetría.",
  "runtime.telemetry.cursor": "Cursor",
  "runtime.telemetry.exported": "Exportados",
  "runtime.telemetry.pending": "Pendientes",
  "runtime.telemetry.droppedHints": "Indicadores descartados",
  "runtime.telemetry.failedAttempts": "Intentos fallidos",
  "runtime.telemetry.note":
    "Un fallo del exportador es degradación de la telemetría, no estado autoritativo de Factory.",
  "runtime.policy.title": "Política del entorno de ejecución",
  "runtime.policy.openSettings": "Abrir configuración",
  "runtime.policy.semanticReview": "Revisión semántica",
  "runtime.policy.modelReceipts": "Recibos MODEL autorizados",
  "runtime.policy.deterministicReceipts": "Solo recibos DETERMINISTIC",
  "runtime.policy.maxOutputTokens": "Máximo de tokens de salida",
  "runtime.policy.updated": "Actualizado",
  "runtime.policy.empty":
    "No se devolvió la configuración del entorno de ejecución.",
  "runtime.policy.note":
    "Las credenciales de proveedores nunca se muestran aquí. Configúralas en Configuración.",
  "runtime.boundaries.title": "Límites de los servicios",
  "runtime.boundaries.hint":
    "No todos los componentes del entorno de ejecución exponen estado",
  "runtime.boundaries.core": "Core",
  "runtime.boundaries.coreVersion": "versión {version}",
  "runtime.boundaries.workers": "Trabajadores",
  "runtime.boundaries.workersDetail":
    "Registrados: {count}; solo el registro de credencial, no un latido",
  "runtime.boundaries.supervisor": "Supervisor",
  "runtime.boundaries.nestedRuntime": "Entorno de ejecución anidado de Podman",
  "runtime.boundaries.broker": "Broker",
  "runtime.boundaries.notExposed": "No expuesto por la API actual de Core",
  "runtime.boundaries.note":
    "Los registros de Compose siguen siendo la fuente de diagnóstico para los componentes sin proyección en Core. Office no infiere detenido, saludable o limpio a partir de un registro ausente en el navegador.",

  "sessions.table.label": "Sesiones de proveedor",
  "sessions.table.session": "Sesión",
  "sessions.table.attempt": "Intento",
  "sessions.table.route": "Trabajador / ruta",
  "sessions.table.state": "Estado",
  "sessions.table.usage": "Uso",
  "sessions.table.cleanup": "Limpieza",
  "sessions.table.updated": "Actualizada",
  "sessions.generation": "generación {generation}",
  "sessions.usage.inputOutput": "{input} entrada / {output} salida",
  "sessions.usage.total": "{tokens} tokens · {cost}",
  "sessions.cleanup.recordedWithheld": "registrado (recibo retenido)",
  "sessions.cleanup.missing": "sin recibo de limpieza",
  "sessions.cleanup.unrecognized": "recibo de limpieza no reconocido",
  "sessions.cleanup.notRecorded": "aún sin registrar",
  "sessions.emptyTitle": "No hay sesiones de proveedor",
  "sessions.emptyDescription":
    "No hay ninguna sesión de proveedor registrada para este ámbito.",

  "modelResolution.operator": "Operador",
  "modelResolution.reason": "Motivo de liberación",
  "modelResolution.reasonPlaceholder": "¿Por qué liberas esta reserva?",
  "modelResolution.release": "Liberar reserva",
  "modelResolution.releasing": "Liberando…",
  "modelResolution.reasonRequired": "Se requiere un motivo.",
  "modelResolution.error": "No se pudo liberar la reserva del modelo.",
  "modelResolution.refreshError":
    "La reserva se liberó, pero falló la actualización. Recarga para confirmar.",
};
