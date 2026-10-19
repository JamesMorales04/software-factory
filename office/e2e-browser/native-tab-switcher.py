#!/usr/bin/env python3
"""Send real Ctrl+Tab switches to the isolated Chromium window via XTest."""

import json
import os
import sys
import time

from Xlib import X, Xatom, display
from Xlib.ext import xtest


def emit(value):
    print(json.dumps(value), flush=True)


def window_process_group(window, connection):
    atom = connection.intern_atom("_NET_WM_PID")
    prop = window.get_full_property(atom, Xatom.CARDINAL)
    if prop is None or len(prop.value) == 0:
        return None
    try:
        return os.getpgid(int(prop.value[0]))
    except (OSError, ValueError):
        return None


def find_chromium_window(root, connection, process_group_id):
    clients = root.get_full_property(
        connection.intern_atom("_NET_CLIENT_LIST"), Xatom.WINDOW
    )
    if clients is None:
        return None
    for window_id in clients.value:
        window = connection.create_resource_object("window", int(window_id))
        try:
            classes = window.get_wm_class() or ()
            is_chromium = any("chrom" in item.lower() for item in classes)
            if is_chromium and window_process_group(window, connection) == process_group_id:
                return window
        except Exception:
            continue
    return None


def active_window_id(root, connection):
    active = root.get_full_property(
        connection.intern_atom("_NET_ACTIVE_WINDOW"), Xatom.WINDOW
    )
    return None if active is None or len(active.value) == 0 else int(active.value[0])


def press_tab(connection, reverse):
    control = connection.keysym_to_keycode(0xFFE3)  # Control_L
    shift = connection.keysym_to_keycode(0xFFE1)  # Shift_L
    tab = connection.keysym_to_keycode(0xFF09)
    sequence = [(X.KeyPress, control)]
    if reverse:
        sequence.append((X.KeyPress, shift))
    sequence.extend(((X.KeyPress, tab), (X.KeyRelease, tab)))
    if reverse:
        sequence.append((X.KeyRelease, shift))
    sequence.append((X.KeyRelease, control))
    for event_type, keycode in sequence:
        xtest.fake_input(connection, event_type, keycode)
    connection.sync()


def main():
    if len(sys.argv) != 2:
        raise RuntimeError("expected Chromium process group ID")
    connection = display.Display()
    root = connection.screen().root
    try:
        process_group_id = int(sys.argv[1])
    except ValueError as error:
        raise RuntimeError("Chromium process group ID must be an integer") from error
    window = find_chromium_window(root, connection, process_group_id)
    if window is None:
        raise RuntimeError(
            f"no X11 Chromium window found in process group {process_group_id}"
        )
    prop = window.get_full_property(
        connection.intern_atom("_NET_WM_PID"), Xatom.CARDINAL
    )
    window_pid = None if prop is None or len(prop.value) == 0 else int(prop.value[0])
    emit(
        {
            "ready": True,
            "windowId": window.id,
            "windowPid": window_pid,
            "processGroupId": process_group_id,
        }
    )

    for line in sys.stdin:
        direction = line.strip()
        if direction not in ("next", "previous"):
            emit({"ok": False, "error": f"unknown tab direction: {direction}"})
            continue
        try:
            window.configure(stack_mode=X.Above)
            window.set_input_focus(X.RevertToParent, X.CurrentTime)
            connection.sync()
            focus = connection.get_input_focus().focus
            time.sleep(0.1)
            press_tab(connection, reverse=direction == "previous")
            emit(
                {
                    "ok": True,
                    "direction": direction,
                    "windowId": window.id,
                    "focusWindowId": focus.id,
                    "activeWindowId": active_window_id(root, connection),
                }
            )
        except Exception as error:
            emit({"ok": False, "error": str(error)})


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        emit({"ready": False, "error": str(error)})
        sys.exit(1)
