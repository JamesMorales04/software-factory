package telemetry

import (
	"context"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	semconv "go.opentelemetry.io/otel/semconv/v1.26.0"
	"go.opentelemetry.io/otel/trace"
	"go.opentelemetry.io/otel/trace/noop"
)

// InitTracing installs a tracer provider and returns a shutdown function.
// Behavior is local-first by construction:
//
//   - No OTEL_EXPORTER_OTLP_ENDPOINT: a no-op provider is used; business
//     code paths never touch the network for telemetry.
//   - Endpoint set but unreachable: spans are exported with a short
//     timeout on a background batcher; export failures are logged and
//     never returned to request handlers.
//
// Callers must invoke shutdown on process exit to flush best-effort spans.
func InitTracing(ctx context.Context, serviceVersion string) (trace.Tracer, func(context.Context)) {
	endpoint := otlpEndpoint()
	if endpoint == "" {
		otel.SetTracerProvider(noop.NewTracerProvider())
		return otel.Tracer("factory-core"), func(context.Context) {}
	}

	exp, err := otlptracehttp.New(ctx,
		otlptracehttp.WithEndpointURL(endpoint),
		otlptracehttp.WithTimeout(2*time.Second),
	)
	if err != nil {
		Logger().Warn("otel exporter unavailable, continuing without export",
			"endpoint", sanitizeEndpoint(endpoint), "error", err.Error())
		otel.SetTracerProvider(noop.NewTracerProvider())
		return otel.Tracer("factory-core"), func(context.Context) {}
	}

	res, _ := resource.New(ctx,
		resource.WithAttributes(
			semconv.ServiceName("factory-core"),
			semconv.ServiceVersion(serviceVersion),
		),
	)
	tp := sdktrace.NewTracerProvider(
		sdktrace.WithBatcher(exp,
			sdktrace.WithExportTimeout(2*time.Second),
			sdktrace.WithMaxExportBatchSize(64),
		),
		sdktrace.WithResource(res),
	)
	otel.SetTracerProvider(tp)

	shutdown := func(ctx context.Context) {
		shCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
		defer cancel()
		if err := tp.Shutdown(shCtx); err != nil {
			Logger().Warn("otel shutdown reported an error", "error", err.Error())
		}
	}
	return otel.Tracer("factory-core"), shutdown
}
