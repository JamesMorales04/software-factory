# Aislamiento, credenciales y límites de confianza

> **Requisito acordado:** sandbox por defecto, host mediante excepción explícita. **Tecnología propuesta:** rootless OCI/Podman (no certificada aún). **Bloqueador F0/F1:** compatibilidad real con Paseo.

## Autoridad

Core posee credenciales y permisos de entrega, políticas, estado y presupuesto. Worker recibe asignaciones con capacidades estrictamente necesarias. Paseo administra sesiones, no decide delivery. Los tests de repositorio, scripts de skills y comandos del agente se consideran **potencialmente no confiables**.

## Workspace

Checkout autocontenido por Run en directorio aislado, commit fijado. No compartir worktrees mutables entre proyectos o Runs. Un worktree Git por sí solo no proporciona seguridad OS. Reviewer consume snapshot candidato sin derechos de escritura.

## Restricciones mínimas

- No HOME, socket contenedor/daemon privilegiado, DB Core, tokens de GitHub u otros proyectos accesibles desde el contexto no confiable.
- Namespace de filesystem/procesos/usuario y recursos limitados; aislamiento de red acorde a modelo/proveedores/dependencias.
- Las capacidades expuestas al LLM son ergonomía; el límite efectivo debe venir del sandbox y de operaciones privilegiadas mediadas por Core.
- Excepciones al host: recurso exacto, identidad, razón, expiración, autorización y auditoría; ninguna excepción `all-host-access` implícita.
- Secretos del runtime: estar en el mismo sandbox que código no confiable **no** demuestra protección. Investigar separación en procesos/contendedores/mediación, autenticación mínima y rutas de shell/MCP no interceptables. Si falla, bloquear, no degradar.

## Pruebas negativas obligatorias

1. El Run no lee archivos de host, otras Actions o repositorios ajenos.
2. El código ejecutado desde el repositorio no puede acceder a credenciales privilegiadas del Core ni al material de autenticación del runtime dentro del modelo de amenaza definido.
3. No es posible ampliar permisos usando instrucciones de skill, shell alternativo o MCP.
4. Paseo puede iniciar, operar, inspeccionarse y cerrarse sin traspasar la frontera.
5. La política de red permite lo requerido y rechaza destinos prohibidos.
6. Restart/cleanup no deja sesiones ni workspaces privilegiados huérfanos.

Los resultados deben ser evidencias reales de una prueba técnica. No afirmar seguridad absoluta ni compatibilidad hasta ejecutarlas.

## Spike obligatorio SEC-01

El plan y criterio de aceptación están en [`paseo-sandbox-spike.md`](paseo-sandbox-spike.md). La capacidad del SDK no acredita por sí misma aislamiento de la ejecución ni inaccesibilidad de tokens en el proceso proveedor.
