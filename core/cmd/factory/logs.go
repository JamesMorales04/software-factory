package main

import (
	"bufio"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"
)

func showLogs(cfg config, args []string) error {
	flags := flag.NewFlagSet("logs", flag.ContinueOnError)
	lines := flags.Int("n", 50, "number of recent lines to show")
	follow := flags.Bool("f", false, "follow new log lines")
	if err := flags.Parse(args); err != nil {
		return err
	}
	service := "core"
	rest := flags.Args()
	if len(rest) == 1 && rest[0] == "worker" {
		service = "worker"
	} else if len(rest) != 0 {
		return errors.New("usage: factory logs [-n lines] [-f] [worker]")
	}
	if *lines < 0 || *lines > 10000 {
		return errors.New("usage: factory logs [-n lines] [-f] [worker]")
	}
	path := filepath.Join(cfg.home, logFileFor(service))
	file, err := os.Open(path)
	if errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("factory log does not exist yet: %s", path)
	}
	if err != nil {
		return fmt.Errorf("open Factory log: %w", err)
	}
	defer func() { _ = file.Close() }()
	if err := tail(file, *lines, os.Stdout); err != nil {
		return err
	}
	if !*follow {
		return nil
	}
	offset, err := file.Seek(0, io.SeekEnd)
	if err != nil {
		return fmt.Errorf("seek Factory log for follow: %w", err)
	}
	buffer := make([]byte, 4096)
	for {
		n, readErr := file.ReadAt(buffer, offset)
		if n > 0 {
			if _, err := os.Stdout.Write(buffer[:n]); err != nil {
				return fmt.Errorf("write followed Factory logs: %w", err)
			}
			offset += int64(n)
		}
		if readErr != nil && !errors.Is(readErr, io.EOF) {
			return fmt.Errorf("read followed Factory log: %w", readErr)
		}
		time.Sleep(250 * time.Millisecond)
	}
}

// logFileFor selects the Core or Worker log file. Only these two names
// exist; anything else is rejected by the caller.
func logFileFor(service string) string {
	if service == "worker" {
		return "worker.log"
	}
	return "factory.log"
}

func tail(file *os.File, lines int, out io.Writer) error {
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return fmt.Errorf("seek Factory log: %w", err)
	}
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 4096), 1024*1024)
	recent := make([]string, 0, lines)
	for scanner.Scan() {
		if lines == 0 {
			continue
		}
		if len(recent) == lines {
			copy(recent, recent[1:])
			recent = recent[:len(recent)-1]
		}
		recent = append(recent, scanner.Text())
	}
	if err := scanner.Err(); err != nil {
		return fmt.Errorf("read Factory log: %w", err)
	}
	for _, line := range recent {
		if _, err := fmt.Fprintln(out, line); err != nil {
			return fmt.Errorf("write Factory logs: %w", err)
		}
	}
	_, err := file.Seek(0, io.SeekEnd)
	return err
}
