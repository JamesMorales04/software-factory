package main

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

func ensurePrivateDir(path string) error {
	if err := os.MkdirAll(path, 0o700); err != nil {
		return fmt.Errorf("create Factory home: %w", err)
	}
	info, err := os.Lstat(path)
	if err != nil {
		return fmt.Errorf("inspect Factory home: %w", err)
	}
	if !info.IsDir() {
		return fmt.Errorf("factory home must be a directory: %s", path)
	}
	if err := os.Chmod(path, 0o700); err != nil {
		return fmt.Errorf("secure Factory home: %w", err)
	}
	return nil
}

func acquireLock(home string) (*os.File, error) {
	if err := ensurePrivateDir(home); err != nil {
		return nil, err
	}
	file, err := os.OpenFile(filepath.Join(home, "factory.lock"), os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, fmt.Errorf("open Factory lock: %w", err)
	}
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX); err != nil {
		_ = file.Close()
		return nil, fmt.Errorf("lock Factory state: %w", err)
	}
	return file, nil
}

func releaseLock(file *os.File) {
	_ = syscall.Flock(int(file.Fd()), syscall.LOCK_UN)
	_ = file.Close()
}

func loadOrCreateToken(path string) (string, error) {
	return loadOrCreateTokenEnv(path, "FACTORY_API_TOKEN")
}

func loadOrCreateTokenEnv(path, envVar string) (string, error) {
	if token := os.Getenv(envVar); token != "" {
		if err := writePrivateFile(path, []byte(token+"\n")); err != nil {
			return "", fmt.Errorf("secure token file: %w", err)
		}
		return token, nil
	}
	data, err := os.ReadFile(path)
	if err == nil {
		info, statErr := os.Lstat(path)
		if statErr != nil || !info.Mode().IsRegular() {
			return "", fmt.Errorf("API token path must be a regular file: %s", path)
		}
		if err := os.Chmod(path, 0o600); err != nil {
			return "", fmt.Errorf("secure API token file: %w", err)
		}
		token := strings.TrimSpace(string(data))
		if token == "" {
			return "", errors.New("API token file is empty")
		}
		return token, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return "", fmt.Errorf("read API token file: %w", err)
	}
	var random [32]byte
	if _, err := rand.Read(random[:]); err != nil {
		return "", fmt.Errorf("generate API token: %w", err)
	}
	token := hex.EncodeToString(random[:])
	if err := writePrivateFile(path, []byte(token+"\n")); err != nil {
		return "", fmt.Errorf("write API token file: %w", err)
	}
	return token, nil
}

func writePrivateFile(path string, contents []byte) error {
	file, err := os.CreateTemp(filepath.Dir(path), ".factory-secret-*")
	if err != nil {
		return err
	}
	temp := file.Name()
	defer func() { _ = os.Remove(temp) }()
	if err := file.Chmod(0o600); err != nil {
		_ = file.Close()
		return err
	}
	if _, err := file.Write(contents); err != nil {
		_ = file.Close()
		return err
	}
	if err := file.Sync(); err != nil {
		_ = file.Close()
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	if err := os.Rename(temp, path); err != nil {
		return err
	}
	return os.Chmod(path, 0o600)
}

func writePrivateJSON(path string, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	return writePrivateFile(path, append(data, '\n'))
}

func readState(path string) (serviceState, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return serviceState{}, err
	}
	var state serviceState
	if err := json.Unmarshal(data, &state); err != nil || state.PID <= 0 || state.Addr == "" {
		return serviceState{}, fmt.Errorf("invalid Factory PID file: %s", path)
	}
	return state, nil
}
