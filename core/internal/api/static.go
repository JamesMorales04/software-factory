package api

import (
	"encoding/json"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"strings"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// projectEvents filters read-only event payloads before they leave the API.
// Legacy source excerpts are dropped; durable rows are untouched.
func projectEvents(events []domain.Event) []domain.Event {
	for i := range events {
		events[i] = events[i].Projected()
	}
	return events
}

// marshalEvent encodes one projected ledger event as a single SSE data line.
func marshalEvent(e domain.Event) (string, error) {
	raw, err := json.Marshal(e.Projected())
	if err != nil {
		return "", err
	}
	return string(raw), nil
}

// serveStaticFile serves a file from the Office build directory with an
// SPA fallback to index.html for client-side routes. It reports whether
// it handled the request. Static assets are intentionally unauthenticated:
// they carry no secrets; every /api/* call behind them is still gated.
func serveStaticFile(w http.ResponseWriter, r *http.Request, root string) bool {
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		return false
	}
	clean := path.Clean("/" + strings.TrimPrefix(r.URL.Path, "/"))
	candidate := filepath.Join(root, filepath.FromSlash(clean))
	if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
		http.ServeFile(w, r, candidate)
		return true
	}
	index := filepath.Join(root, "index.html")
	if _, err := os.Stat(index); err != nil {
		return false
	}
	http.ServeFile(w, r, index)
	return true
}
