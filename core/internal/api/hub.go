// Package api exposes the authenticated local HTTP boundary of Factory
// Core. Handlers translate transport concerns (auth, correlation, JSON
// shapes, SSE framing) and delegate every rule to domain and store.
package api

import (
	"sync"

	"github.com/JamesMorales04/software-factory/core/internal/domain"
)

// Hub is a tiny in-process event broadcaster for SSE live updates. Replay
// correctness never depends on it: reconnecting clients always re-read the
// missed suffix from the durable ledger with their cursor first.
type Hub struct {
	mu   sync.Mutex
	subs map[chan domain.Event]struct{}
}

// NewHub builds an empty broadcaster.
func NewHub() *Hub { return &Hub{subs: map[chan domain.Event]struct{}{}} }

// Publish delivers e to current subscribers without blocking the writer:
// a slow consumer drops the live push and catches up via replay cursor.
func (h *Hub) Publish(e domain.Event) {
	h.mu.Lock()
	defer h.mu.Unlock()
	for ch := range h.subs {
		select {
		case ch <- e:
		default:
		}
	}
}

// Subscribe returns a channel and an unsubscribe function.
func (h *Hub) Subscribe() (<-chan domain.Event, func()) {
	ch := make(chan domain.Event, 64)
	h.mu.Lock()
	h.subs[ch] = struct{}{}
	h.mu.Unlock()
	return ch, func() {
		h.mu.Lock()
		delete(h.subs, ch)
		close(ch)
		h.mu.Unlock()
	}
}
