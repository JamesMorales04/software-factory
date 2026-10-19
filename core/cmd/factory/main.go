// Command factory manages one local Factory Core process and serves its
// built Office bundle. It intentionally does not supervise agents or workers.
package main

import (
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strconv"
)

const defaultAddr = "127.0.0.1:8471"

type config struct {
	home   string
	addr   string
	binary string
	webDir string
}

type serviceState struct {
	PID        int    `json:"pid"`
	Addr       string `json:"addr"`
	Executable string `json:"executable"`
}

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "factory:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	if len(args) == 0 {
		return usageError()
	}
	cfg, err := loadConfig()
	if err != nil {
		return err
	}
	switch args[0] {
	case "start":
		if len(args) != 1 {
			return usageError()
		}
		return start(cfg)
	case "stop":
		if len(args) != 1 {
			return usageError()
		}
		return stop(cfg)
	case "status":
		if len(args) != 1 {
			return usageError()
		}
		return status(cfg)
	case "logs":
		return showLogs(cfg, args[1:])
	case "help", "--help", "-h":
		fmt.Print("Usage: factory {start|stop|status|logs [-n lines] [-f]}\n")
		return nil
	default:
		return usageError()
	}
}

func usageError() error {
	return errors.New("usage: factory {start|stop|status|logs [-n lines] [-f]}")
}

func loadConfig() (config, error) {
	home := os.Getenv("FACTORY_HOME")
	if home == "" {
		if dataHome := os.Getenv("XDG_DATA_HOME"); dataHome != "" {
			home = filepath.Join(dataHome, "software-factory")
		} else {
			userHome, err := os.UserHomeDir()
			if err != nil {
				return config{}, fmt.Errorf("resolve user home: %w", err)
			}
			home = filepath.Join(userHome, ".local", "share", "software-factory")
		}
	}
	home, err := filepath.Abs(home)
	if err != nil {
		return config{}, fmt.Errorf("resolve Factory home: %w", err)
	}

	addr := os.Getenv("FACTORY_ADDR")
	if addr == "" {
		addr = defaultAddr
	}
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		return config{}, fmt.Errorf("FACTORY_ADDR must be host:port: %w", err)
	}
	if host != "localhost" {
		ip := net.ParseIP(host)
		if ip == nil || !ip.IsLoopback() {
			return config{}, errors.New("FACTORY_ADDR must use a loopback host to keep the local API private")
		}
	}
	portNumber, err := strconv.Atoi(port)
	if err != nil || portNumber < 1 || portNumber > 65535 {
		return config{}, errors.New("FACTORY_ADDR must use a valid TCP port")
	}

	exe, err := os.Executable()
	if err != nil {
		return config{}, fmt.Errorf("locate factory executable: %w", err)
	}
	exe, err = filepath.Abs(exe)
	if err != nil {
		return config{}, fmt.Errorf("resolve factory executable: %w", err)
	}
	exeDir := filepath.Dir(exe)

	coreBin := os.Getenv("FACTORY_CORE_BIN")
	if coreBin == "" {
		coreBin = filepath.Join(exeDir, "factory-core")
	}
	coreBin, err = filepath.Abs(coreBin)
	if err != nil {
		return config{}, fmt.Errorf("resolve Core executable: %w", err)
	}

	webDir := os.Getenv("FACTORY_WEB_DIR")
	if webDir == "" {
		webDir = filepath.Join(exeDir, "office", "dist")
	}
	webDir, err = filepath.Abs(webDir)
	if err != nil {
		return config{}, fmt.Errorf("resolve Office build: %w", err)
	}

	return config{home: home, addr: addr, binary: coreBin, webDir: webDir}, nil
}
