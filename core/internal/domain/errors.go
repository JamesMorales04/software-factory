package domain

import "errors"

// Typed domain/store errors so adapters map them to stable API codes
// without string matching.
var (
	ErrNotFound          = errors.New("not found")
	ErrConflict          = errors.New("conflict")
	ErrInvalidTransition = errors.New("invalid status transition")
	ErrUnauthorized      = errors.New("unauthorized")
	ErrInvalidInput      = errors.New("invalid input")
	// S2 fencing errors: a superseded or untimely Worker message must
	// never overwrite authoritative state.
	ErrStaleGeneration = errors.New("stale lease generation")
	ErrLeaseExpired    = errors.New("lease expired")
	ErrRunCancelled    = errors.New("run cancelled")
)
