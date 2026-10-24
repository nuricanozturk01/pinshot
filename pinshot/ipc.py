"""Single-instance guard + command channel (`pinshot --capture` talks to the running tray app)."""
from __future__ import annotations

import getpass

from PySide6.QtCore import QObject, Signal
from PySide6.QtNetwork import QLocalServer, QLocalSocket


def _name() -> str:
    try:
        user = getpass.getuser()
    except Exception:  # noqa: BLE001
        user = "user"
    return f"pinshot-{user}"


def send_command(cmd: str, timeout_ms: int = 800) -> bool:
    """Deliver `cmd` to a running instance. False when none is running."""
    sock = QLocalSocket()
    sock.connectToServer(_name())
    if not sock.waitForConnected(timeout_ms):
        return False
    sock.write(cmd.encode("utf-8") + b"\n")
    sock.flush()
    sock.waitForBytesWritten(timeout_ms)
    sock.disconnectFromServer()
    return True


class CommandServer(QObject):
    command = Signal(str)

    def __init__(self) -> None:
        super().__init__()
        self.server = QLocalServer(self)
        self.server.newConnection.connect(self._on_conn)

    def listen(self) -> bool:
        QLocalServer.removeServer(_name())     # stale socket from a crash
        return self.server.listen(_name())

    def _on_conn(self) -> None:
        while self.server.hasPendingConnections():
            sock = self.server.nextPendingConnection()
            sock.readyRead.connect(lambda s=sock: self._read(s))
            sock.disconnected.connect(sock.deleteLater)

    def _read(self, sock: QLocalSocket) -> None:
        for line in bytes(sock.readAll()).decode("utf-8", "replace").splitlines():
            if line.strip():
                self.command.emit(line.strip())
