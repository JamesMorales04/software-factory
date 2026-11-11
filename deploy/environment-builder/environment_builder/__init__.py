"""Containerized Nix/devenv environment builder for Software Factory.

The package is executed inside the builder image. It validates a Core-frozen
``EnvironmentSpec``/``EnvironmentPlan`` pair, runs Nix/devenv without host
network or credentials, and emits a typed ``EnvironmentBuildReceipt`` plus
bounded build, SBOM, license and sandbox evidence.

Security status: ``TRUSTED_LOCAL``. The builder is not an isolation
certification and never grants authority to project or model input.
"""

SCHEMA_VERSION = 1
BUILDER_POLICY_REVISION = "nix-devenv-environment-v1"
LICENSE_POLICY_REVISION = "nix-devenv-license-v1"
DEFAULT_PLATFORM = "x86_64-linux"
