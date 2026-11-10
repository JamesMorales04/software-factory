//! Curated, source-cited technology packs used when no persisted revision is
//! available. The catalog is intentionally bounded and deterministic; every
//! entry cites official documentation and carries a review deadline.
//!
//! A catalog entry is context only. It never grants tools, authority or a
//! planning lane, and it cannot replace an owner decision.

use crate::planning::SkillPack;

pub const BUILTIN_LAST_VERIFIED_AT: &str = "2026-11-09T00:00:00Z";
pub const BUILTIN_REVIEW_AFTER: &str = "2027-02-07T00:00:00Z";

/// Canonical technology names covered by the curated catalog. The list is used
/// by the refresh scheduler and by coverage tests; keep it sorted and free of
/// aliases.
pub const BUILTIN_TECHNOLOGIES: &[&str] = &[
    "actix-web",
    "angular",
    "axum",
    "bun",
    "bundler",
    "cargo",
    "clap",
    "composer",
    "container",
    "csharp",
    "deno",
    "diesel",
    "django",
    "dotnet",
    "electron",
    "fastapi",
    "flask",
    "github-actions",
    "go",
    "gradle",
    "hyper",
    "java",
    "jest",
    "kotlin",
    "kubernetes",
    "laravel",
    "maven",
    "migrations",
    "mysql",
    "next",
    "node",
    "npm",
    "nuget",
    "php",
    "playwright",
    "pnpm",
    "podman-compose",
    "postgresql",
    "pytest",
    "python",
    "rails",
    "react",
    "redis",
    "reqwest",
    "rocket",
    "ruff",
    "ruby",
    "rust",
    "serde",
    "sqlite",
    "sqlx",
    "svelte",
    "swift",
    "tauri",
    "terraform",
    "tokio",
    "tonic",
    "tracing",
    "typescript",
    "uv",
    "vite",
    "vitest",
    "vue",
    "yarn",
];

fn pack(
    technology: &str,
    sources: &[&str],
    best_practices: &[&str],
    anti_patterns: &[&str],
    security_notes: &[&str],
    commands: &[&str],
) -> SkillPack {
    SkillPack {
        technology: technology.into(),
        version: "current".into(),
        source_urls: sources.iter().map(|source| (*source).to_string()).collect(),
        last_verified_at: BUILTIN_LAST_VERIFIED_AT.into(),
        review_after: BUILTIN_REVIEW_AFTER.into(),
        best_practices: best_practices
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        anti_patterns: anti_patterns
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        commands: commands.iter().map(|value| (*value).to_string()).collect(),
        security_notes: security_notes
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        deprecations: Vec::new(),
        license: "Official documentation terms; verify upstream license.".into(),
        provenance: "Factory curated pack built from the cited official documentation.".into(),
        content_sha256: String::new(),
    }
}

/// Curated packs for the technologies the Factory supports. These are not a
/// substitute for refreshing upstream documentation; `review_after` forces a
/// periodic review.
pub fn builtin_skill_pack(technology: &str) -> Option<SkillPack> {
    let pack = match technology.to_ascii_lowercase().as_str() {
        "actix-web" => pack(
            "actix-web",
            &["https://actix.rs/docs/"],
            &[
                "Keep handlers small and move domain decisions into typed services.",
                "Configure explicit body limits and request timeouts.",
            ],
            &["Do not trust extractor input before validation."],
            &["Enforce authorization in the service layer, not only in routing."],
            &["cargo test"],
        ),
        "angular" => pack(
            "angular",
            &["https://angular.dev/overview"],
            &[
                "Prefer standalone components and typed reactive forms.",
                "Keep HTTP access behind typed services.",
            ],
            &["Do not bypass sanitization for untrusted template content."],
            &["Treat route guards as UX only; authorize on the server."],
            &["npm run build", "npm test"],
        ),
        "axum" => pack(
            "axum",
            &["https://docs.rs/axum/latest/axum/"],
            &[
                "Keep handlers thin and domain decisions in typed services.",
                "Return explicit status codes and typed error bodies.",
            ],
            &["Do not trust request bodies before validation."],
            &["Enforce authorization in the service boundary."],
            &["cargo test"],
        ),
        "bun" => pack(
            "bun",
            &["https://bun.sh/docs"],
            &[
                "Commit the lockfile and install with `--frozen-lockfile`.",
                "Keep runtime-specific APIs behind a small adapter.",
            ],
            &["Do not run install scripts from unreviewed packages."],
            &["Review install scripts and package sources before execution."],
            &["bun install --frozen-lockfile", "bun test"],
        ),
        "bundler" => pack(
            "bundler",
            &["https://bundler.io/docs.html"],
            &[
                "Commit `Gemfile.lock` for applications.",
                "Use `bundle exec` so checks use the locked dependency set.",
            ],
            &["Do not resolve new gems implicitly during a frozen check."],
            &["Verify the configured gem sources before installation."],
            &["bundle install --deployment", "bundle exec rake test"],
        ),
        "cargo" => pack(
            "cargo",
            &["https://doc.rust-lang.org/cargo/"],
            &[
                "Commit `Cargo.lock` for applications and use `--locked` in checks.",
                "Vendor dependencies for offline, reproducible builds.",
            ],
            &["Do not let a check command resolve new dependencies implicitly."],
            &["Verify registry and source replacement before an offline build."],
            &["cargo metadata --locked", "cargo vendor --locked vendor"],
        ),
        "clap" => pack(
            "clap",
            &["https://docs.rs/clap/latest/clap/"],
            &["Declare arguments with typed parsers and help text."],
            &["Return non-zero exit codes for invalid input."],
            &["Do not echo secrets in error output."],
            &["cargo test"],
        ),
        "composer" => pack(
            "composer",
            &["https://getcomposer.org/doc/"],
            &[
                "Commit `composer.lock` and install with `--no-dev` in production.",
                "Validate the lockfile before deployment.",
            ],
            &["Do not install packages from unreviewed repositories."],
            &["Review Composer scripts before executing them."],
            &["composer validate --strict", "composer install --no-dev"],
        ),
        "container" => pack(
            "container",
            &[
                "https://docs.podman.io/",
                "https://podman-desktop.io/docs/compose",
            ],
            &[
                "Run rootless with an explicit network and least privilege.",
                "Pin image digests for reproducible deployments.",
            ],
            &[
                "Do not mount the host Podman socket or use privileged mode.",
                "Do not add host networking as a shortcut.",
            ],
            &["Treat container images and mounts as a trust boundary."],
            &["podman compose config"],
        ),
        "csharp" => pack(
            "csharp",
            &["https://learn.microsoft.com/en-us/dotnet/csharp/"],
            &[
                "Enable nullable reference types for new code.",
                "Keep async code awaited end to end.",
            ],
            &["Do not deserialize untrusted input into permissive object graphs."],
            &["Validate model binding and authorization on the server."],
            &["dotnet build", "dotnet test"],
        ),
        "deno" => pack(
            "deno",
            &["https://docs.deno.com/"],
            &[
                "Use explicit permission flags for runtime capabilities.",
                "Commit the lockfile and check with `deno check`.",
            ],
            &["Do not run with all permissions when a narrower set works."],
            &["Treat remote imports as supply-chain inputs that need review."],
            &["deno check", "deno test"],
        ),
        "diesel" => pack(
            "diesel",
            &["https://diesel.rs/guides/"],
            &[
                "Use versioned migrations and typed queries.",
                "Keep database errors typed and recoverable.",
            ],
            &["Do not interpolate SQL strings from request data."],
            &["Apply least privilege to the database account."],
            &["diesel migration run", "cargo test"],
        ),
        "django" => pack(
            "django",
            &["https://docs.djangoproject.com/en/stable/"],
            &[
                "Keep `DEBUG` disabled outside local development.",
                "Use the ORM and migrations instead of ad-hoc schema changes.",
            ],
            &["Do not trust request data in raw SQL or template markup."],
            &[
                "Keep `SECRET_KEY` out of source control.",
                "Enable CSRF and secure cookie settings.",
            ],
            &["python manage.py check --deploy", "python manage.py test"],
        ),
        "dotnet" => pack(
            "dotnet",
            &["https://learn.microsoft.com/en-us/dotnet/"],
            &[
                "Pin the SDK with `global.json` for reproducible builds.",
                "Use `dotnet test` and treat warnings as review findings.",
            ],
            &["Do not commit secrets to `appsettings` files."],
            &["Use the platform secret store for credentials."],
            &["dotnet build", "dotnet test"],
        ),
        "electron" => pack(
            "electron",
            &["https://www.electronjs.org/docs/latest/"],
            &[
                "Enable context isolation and disable Node integration in renderers.",
                "Load remote content only through a narrow, validated bridge.",
            ],
            &["Do not expose the full Node API to renderer code."],
            &["Treat renderer content as untrusted; enforce a strict CSP."],
            &["npm run build", "npm test"],
        ),
        "fastapi" => pack(
            "fastapi",
            &["https://fastapi.tiangolo.com/"],
            &[
                "Declare request and response models explicitly.",
                "Keep authorization checks in dependencies or services.",
            ],
            &["Do not return raw exception text to clients."],
            &["Treat generated OpenAPI as a contract to review."],
            &["python3 -m pytest"],
        ),
        "flask" => pack(
            "flask",
            &["https://flask.palletsprojects.com/en/stable/"],
            &[
                "Use application factories and explicit configuration.",
                "Validate request data before passing it to domain code.",
            ],
            &["Do not run the development server or debug mode in production."],
            &[
                "Keep `SECRET_KEY` out of source control.",
                "Enable CSRF protection for browser forms.",
            ],
            &["python3 -m pytest"],
        ),
        "github-actions" => pack(
            "github-actions",
            &["https://docs.github.com/en/actions"],
            &[
                "Pin third-party actions to a full commit SHA.",
                "Grant the workflow token the minimum required permissions.",
            ],
            &["Do not interpolate untrusted event text directly into shell commands."],
            &["Treat pull-request workflows as untrusted code execution."],
            &["gh workflow list"],
        ),
        "go" => pack(
            "go",
            &["https://go.dev/doc/"],
            &["Use `go test ./...` and `go vet ./...` as deterministic checks."],
            &["Do not ignore returned errors."],
            &["Pin module versions and verify checksums."],
            &["go test ./..."],
        ),
        "gradle" => pack(
            "gradle",
            &["https://docs.gradle.org/current/userguide/userguide.html"],
            &[
                "Commit the Gradle wrapper and use it for checks.",
                "Keep build logic in convention plugins rather than copy-paste.",
            ],
            &["Do not execute unreviewed build scripts or plugins."],
            &["Verify plugin and repository sources before a build."],
            &["./gradlew build", "./gradlew test"],
        ),
        "hyper" => pack(
            "hyper",
            &["https://hyper.rs/guide/"],
            &[
                "Apply body limits and timeouts to every server path.",
                "Keep protocol plumbing separate from domain decisions.",
            ],
            &["Do not buffer unbounded request bodies."],
            &["Terminate TLS correctly and validate peer identity where required."],
            &["cargo test"],
        ),
        "java" => pack(
            "java",
            &[
                "https://docs.oracle.com/en/java/",
                "https://dev.java/learn/",
            ],
            &[
                "Use the current LTS toolchain pinned by the build.",
                "Prefer explicit types and immutable data at module boundaries.",
            ],
            &["Do not deserialize untrusted object graphs."],
            &["Verify dependency sources and signatures."],
            &["mvn test", "./gradlew test"],
        ),
        "jest" => pack(
            "jest",
            &["https://jestjs.io/docs/getting-started"],
            &["Keep tests deterministic and independent."],
            &["Do not depend on execution order."],
            &["Never commit live credentials to test fixtures."],
            &["npx jest --runInBand"],
        ),
        "kotlin" => pack(
            "kotlin",
            &["https://kotlinlang.org/docs/home.html"],
            &[
                "Use null-safety deliberately at API boundaries.",
                "Keep coroutine scopes tied to a bounded lifecycle.",
            ],
            &["Do not block a coroutine dispatcher with synchronous I/O."],
            &["Validate untrusted data even when the type system is strict."],
            &["./gradlew test"],
        ),
        "kubernetes" => pack(
            "kubernetes",
            &["https://kubernetes.io/docs/home/"],
            &[
                "Apply least-privilege RBAC and explicit network policies.",
                "Set resource requests and limits for every workload.",
            ],
            &["Do not run privileged pods or mount the host runtime socket."],
            &["Treat Secrets as sensitive; enable encryption at rest."],
            &[
                "kubectl apply --dry-run=client -f k8s/",
                "kubectl config current-context",
            ],
        ),
        "laravel" => pack(
            "laravel",
            &["https://laravel.com/docs"],
            &[
                "Use form requests and policies for validation and authorization.",
                "Keep configuration in environment-specific config files.",
            ],
            &["Do not mass-assign unfiltered request input."],
            &[
                "Keep `APP_KEY` and credentials out of source control.",
                "Enable CSRF protection for browser routes.",
            ],
            &["php artisan test"],
        ),
        "maven" => pack(
            "maven",
            &["https://maven.apache.org/guides/index.html"],
            &[
                "Pin plugin and dependency versions in the POM.",
                "Use the Maven wrapper for reproducible checks.",
            ],
            &["Do not resolve artifacts from unreviewed repositories."],
            &["Verify checksums and repository configuration before a build."],
            &["./mvnw test"],
        ),
        "migrations" => pack(
            "migrations",
            &["https://www.sqlite.org/lang_altertable.html"],
            &[
                "Use versioned, forward-only migrations.",
                "Test migrations against a fresh database and an upgraded database.",
            ],
            &["Do not edit an applied migration in place."],
            &["Treat destructive schema changes as owner-reserved."],
            &["cargo test"],
        ),
        "mysql" => pack(
            "mysql",
            &["https://dev.mysql.com/doc/"],
            &[
                "Use prepared statements and an explicit transaction boundary.",
                "Keep schema migrations versioned and reviewed.",
            ],
            &["Do not connect as an administrative account from the application."],
            &["Use TLS and least-privilege accounts for remote connections."],
            &["mysql --version"],
        ),
        "next" => pack(
            "next",
            &["https://nextjs.org/docs"],
            &["Keep server-only secrets out of client bundles."],
            &["Validate route inputs on the server."],
            &["Treat server actions as authenticated endpoints."],
            &["npm run build"],
        ),
        "node" => pack(
            "node",
            &["https://nodejs.org/docs/latest/api/"],
            &["Pin the runtime version in the repository."],
            &["Do not run untrusted scripts with ambient credentials."],
            &["Treat the runtime as a public client for network calls."],
            &["npm test"],
        ),
        "npm" => pack(
            "npm",
            &["https://docs.npmjs.com/"],
            &["Commit the lockfile and use `npm ci` for deterministic installs."],
            &["Do not install packages from unreviewed registries."],
            &["Review install scripts before execution."],
            &["npm ci"],
        ),
        "nuget" => pack(
            "nuget",
            &["https://learn.microsoft.com/en-us/nuget/"],
            &[
                "Commit the lock file and restore with locked mode.",
                "Pin package versions explicitly.",
            ],
            &["Do not restore from unreviewed package sources."],
            &["Review package sources and signing before restore."],
            &["dotnet restore --locked-mode"],
        ),
        "php" => pack(
            "php",
            &["https://www.php.net/docs.php"],
            &[
                "Use prepared statements and explicit input validation.",
                "Enable strict types in new files where practical.",
            ],
            &["Do not use `eval` or unserialize untrusted input."],
            &["Disable detailed error display in production."],
            &["php -l", "composer validate --strict"],
        ),
        "playwright" => pack(
            "playwright",
            &["https://playwright.dev/docs/intro"],
            &[
                "Prefer role and label locators over CSS selectors.",
                "Test the real application state and observable outcomes.",
            ],
            &["Do not use fixed sleeps as synchronization."],
            &["Never store credentials in test source or traces."],
            &["npx playwright test"],
        ),
        "pnpm" => pack(
            "pnpm",
            &["https://pnpm.io/"],
            &[
                "Commit `pnpm-lock.yaml` and install with `--frozen-lockfile`.",
                "Declare workspace boundaries explicitly.",
            ],
            &["Do not install packages from unreviewed registries."],
            &["Review lifecycle scripts before execution."],
            &["pnpm install --frozen-lockfile", "pnpm test"],
        ),
        "podman-compose" => pack(
            "podman-compose",
            &[
                "https://docs.podman.io/",
                "https://podman-desktop.io/docs/compose",
            ],
            &[
                "Run rootless with explicit networks and least privilege.",
                "Pin image digests for reproducible deployments.",
            ],
            &[
                "Do not mount the host Podman socket or use privileged mode.",
                "Do not add host networking as a shortcut.",
            ],
            &["Treat Compose files, images and volumes as a trust boundary."],
            &["podman compose config"],
        ),
        "postgresql" => pack(
            "postgresql",
            &["https://www.postgresql.org/docs/"],
            &[
                "Use parameterized queries and explicit transactions.",
                "Keep schema changes in versioned migrations.",
            ],
            &["Do not connect as a superuser from the application."],
            &["Use TLS and least-privilege roles for remote connections."],
            &["pg_isready", "psql --version"],
        ),
        "pytest" => pack(
            "pytest",
            &["https://docs.pytest.org/"],
            &["Prefer fixtures with explicit scope and cleanup."],
            &["Do not depend on test execution order."],
            &["Never embed live credentials in fixtures."],
            &["python3 -m pytest"],
        ),
        "python" => pack(
            "python",
            &["https://docs.python.org/3/"],
            &[
                "Use type hints at module and public function boundaries.",
                "Keep dependencies pinned for reproducible environments.",
            ],
            &["Do not use mutable default arguments."],
            &["Validate untrusted input before parsing or executing it."],
            &["python3 -m pytest"],
        ),
        "rails" => pack(
            "rails",
            &["https://guides.rubyonrails.org/"],
            &[
                "Use strong parameters and explicit authorization policies.",
                "Keep credentials in the encrypted credentials store.",
            ],
            &["Do not mass-assign unfiltered request parameters."],
            &["Enable CSRF protection and secure cookie settings."],
            &["bin/rails test"],
        ),
        "react" => pack(
            "react",
            &["https://react.dev/"],
            &[
                "Keep state close to the component that owns it.",
                "Use accessible semantic elements before adding ARIA.",
            ],
            &["Do not derive authoritative state from presentation state."],
            &["Render untrusted text as text, never as raw HTML."],
            &["npm test"],
        ),
        "redis" => pack(
            "redis",
            &["https://redis.io/docs/latest/"],
            &[
                "Use explicit key naming and bounded value sizes.",
                "Set an eviction and persistence policy that matches the workload.",
            ],
            &["Do not use Redis as the sole durable record for critical state."],
            &["Require authentication/TLS and never expose Redis publicly."],
            &["redis-cli ping"],
        ),
        "reqwest" => pack(
            "reqwest",
            &["https://docs.rs/reqwest/latest/reqwest/"],
            &[
                "Set explicit timeouts and bound response bodies.",
                "Keep credentials out of URLs and logs.",
            ],
            &["Do not disable TLS verification outside a narrow test fixture."],
            &["Validate redirects and destination hosts for SSRF-sensitive calls."],
            &["cargo test"],
        ),
        "rocket" => pack(
            "rocket",
            &["https://rocket.rs/guide/"],
            &[
                "Declare request guards and typed responders explicitly.",
                "Configure body limits and fairings deliberately.",
            ],
            &["Do not trust request data before a guard validates it."],
            &["Keep secrets in Rocket configuration, not source."],
            &["cargo test"],
        ),
        "ruff" => pack(
            "ruff",
            &["https://docs.astral.sh/ruff/"],
            &["Run lint and format checks in CI."],
            &["Do not blanket-disable rules without a scoped rationale."],
            &["Treat lint findings as review findings."],
            &["python3 -m ruff check ."],
        ),
        "ruby" => pack(
            "ruby",
            &[
                "https://docs.ruby-lang.org/en/",
                "https://www.ruby-lang.org/en/documentation/",
            ],
            &[
                "Keep dependencies locked with Bundler.",
                "Prefer small objects with explicit responsibilities.",
            ],
            &["Do not `eval` or unmarshal untrusted input."],
            &["Treat dynamic dispatch from request data as a security risk."],
            &["bundle exec rake test"],
        ),
        "rust" => pack(
            "rust",
            &[
                "https://doc.rust-lang.org/book/",
                "https://doc.rust-lang.org/cargo/",
                "https://rust-lang.github.io/rust-clippy/",
            ],
            &[
                "Prefer ownership and explicit error types over panics in library code.",
                "Use `cargo fmt`, Clippy with `-D warnings`, and tests as deterministic checks.",
            ],
            &[
                "Do not add `unsafe` without a documented safety argument.",
                "Do not hide fallible operations behind `unwrap` in production paths.",
            ],
            &["Treat dependency advisories as blocking findings."],
            &[
                "cargo fmt --check",
                "cargo clippy -- -D warnings",
                "cargo test",
            ],
        ),
        "serde" => pack(
            "serde",
            &["https://serde.rs/"],
            &[
                "Use `deny_unknown_fields` for security-sensitive input contracts.",
                "Keep wire types separate from domain types where they differ.",
            ],
            &["Do not deserialize unbounded or deeply nested untrusted input."],
            &["Validate deserialized values before they reach a privileged action."],
            &["cargo test"],
        ),
        "sqlite" => pack(
            "sqlite",
            &["https://www.sqlite.org/docs.html"],
            &["Use WAL mode and explicit transactions for concurrent writers."],
            &["Do not rely on implicit column affinity for validation."],
            &["Treat database files as sensitive local state."],
            &["cargo test"],
        ),
        "sqlx" => pack(
            "sqlx",
            &["https://docs.rs/sqlx/latest/sqlx/"],
            &[
                "Use versioned migrations and explicit transactions.",
                "Bind parameters instead of interpolating SQL.",
            ],
            &["Do not run unchecked schema changes against production data."],
            &["Treat database errors as typed, recoverable outcomes."],
            &["cargo test"],
        ),
        "svelte" => pack(
            "svelte",
            &["https://svelte.dev/docs"],
            &[
                "Keep reactive state explicit and test observable behavior.",
                "Use accessible native elements before ARIA.",
            ],
            &["Do not render untrusted content with `{@html}`."],
            &["Sanitize any HTML that must be rendered and validate on the server."],
            &["npm run build", "npm test"],
        ),
        "swift" => pack(
            "swift",
            &[
                "https://www.swift.org/documentation/",
                "https://developer.apple.com/documentation/swift",
            ],
            &[
                "Use Swift Package Manager for reproducible dependency resolution.",
                "Handle errors explicitly with typed results.",
            ],
            &["Do not force-unwrap values from untrusted input."],
            &["Keep credentials out of logs and source."],
            &["swift build", "swift test"],
        ),
        "tauri" => pack(
            "tauri",
            &["https://v2.tauri.app/"],
            &[
                "Use a strict content security policy for the webview.",
                "Expose a narrow command allowlist to the frontend.",
            ],
            &["Do not expose arbitrary filesystem or shell commands to the webview."],
            &["Treat remote content as untrusted and validate command input."],
            &["npm run tauri build", "cargo test"],
        ),
        "terraform" => pack(
            "terraform",
            &["https://developer.hashicorp.com/terraform/docs"],
            &[
                "Keep state in an encrypted remote backend with locking.",
                "Review `terraform plan` before applying a change.",
            ],
            &["Do not commit state files or credentials to source control."],
            &["Treat state as sensitive because it can contain secrets."],
            &["terraform fmt -check", "terraform validate"],
        ),
        "tokio" => pack(
            "tokio",
            &["https://tokio.rs/"],
            &[
                "Bound concurrency and cancel work on shutdown.",
                "Never hold a lock across an await unless the design requires it.",
            ],
            &["Do not spawn unbounded background tasks."],
            &["Propagate cancellation to external calls."],
            &["cargo test"],
        ),
        "tonic" => pack(
            "tonic",
            &["https://docs.rs/tonic/latest/tonic/"],
            &[
                "Declare bounded message sizes and timeouts.",
                "Keep authentication and authorization interceptors explicit.",
            ],
            &["Do not trust metadata before validation."],
            &["Use TLS and verify peer identity for remote gRPC."],
            &["cargo test"],
        ),
        "tracing" => pack(
            "tracing",
            &["https://docs.rs/tracing/latest/tracing/"],
            &[
                "Use structured fields and stable span names.",
                "Keep logs bounded and redact secrets at the writer.",
            ],
            &["Do not log raw prompts, tokens or personal data."],
            &["Treat logs as an observability surface with access controls."],
            &["cargo test"],
        ),
        "typescript" => pack(
            "typescript",
            &["https://www.typescriptlang.org/docs/"],
            &[
                "Keep strict type checking enabled for application code.",
                "Prefer explicit domain types at API boundaries.",
            ],
            &["Do not silence errors with `any` or blanket `ts-ignore`."],
            &["Treat untrusted JSON as `unknown` until validated."],
            &["npm run typecheck"],
        ),
        "uv" => pack(
            "uv",
            &["https://docs.astral.sh/uv/"],
            &["Commit the lockfile and use locked syncs."],
            &["Do not resolve new dependencies during a frozen check."],
            &["Verify package sources before installation."],
            &["uv sync --locked"],
        ),
        "vite" => pack(
            "vite",
            &["https://vite.dev/guide/"],
            &["Keep build-time environment values non-secret."],
            &["Do not expose server credentials through `VITE_` variables."],
            &["Treat client bundles as public artifacts."],
            &["npm run build"],
        ),
        "vitest" => pack(
            "vitest",
            &["https://vitest.dev/guide/"],
            &["Keep tests deterministic and independent."],
            &["Do not depend on shared mutable module state."],
            &["Never commit live credentials to test fixtures."],
            &["npx vitest run"],
        ),
        "vue" => pack(
            "vue",
            &["https://vuejs.org/guide/"],
            &[
                "Prefer typed props and explicit emits.",
                "Keep server state behind a typed API client.",
            ],
            &["Do not render untrusted content with `v-html`."],
            &["Validate and authorize on the server; client checks are UX only."],
            &["npm run build", "npm test"],
        ),
        "yarn" => pack(
            "yarn",
            &["https://yarnpkg.com/getting-started"],
            &[
                "Commit the lockfile and use an immutable install in checks.",
                "Pin the package manager version in the repository.",
            ],
            &["Do not install packages from unreviewed registries."],
            &["Review lifecycle scripts before execution."],
            &["yarn install --immutable", "yarn test"],
        ),
        // Compatibility aliases used by older detection output or callers.
        // Each alias keeps its requested technology name so persisted revisions
        // and planning needs cannot silently diverge.
        "docker" => pack(
            "docker",
            &[
                "https://docs.podman.io/",
                "https://podman-desktop.io/docs/compose",
            ],
            &[
                "Run rootless with an explicit network and least privilege.",
                "Pin image digests for reproducible deployments.",
            ],
            &["Do not mount the host runtime socket or use privileged mode."],
            &["Treat container images and mounts as a trust boundary."],
            &["podman compose config"],
        ),
        "docker-compose" => pack(
            "docker-compose",
            &[
                "https://docs.podman.io/",
                "https://podman-desktop.io/docs/compose",
            ],
            &[
                "Run rootless with explicit networks and least privilege.",
                "Pin image digests for reproducible deployments.",
            ],
            &[
                "Do not mount the host Podman socket or use privileged mode.",
                "Do not add host networking as a shortcut.",
            ],
            &["Treat Compose files, images and volumes as a trust boundary."],
            &["podman compose config"],
        ),
        "podman" => pack(
            "podman",
            &[
                "https://docs.podman.io/",
                "https://podman-desktop.io/docs/compose",
            ],
            &[
                "Run rootless with explicit networks and least privilege.",
                "Pin image digests for reproducible deployments.",
            ],
            &[
                "Do not mount the host Podman socket or use privileged mode.",
                "Do not add host networking as a shortcut.",
            ],
            &["Treat container images and volumes as a trust boundary."],
            &["podman compose config"],
        ),
        _ => return None,
    };
    Some(pack)
}
