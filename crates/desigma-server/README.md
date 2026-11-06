# desigma-server

A headless, always-on host for [live collaboration](../../docs/collaboration.md). It keeps one
`.desigma` file shared permanently, with the same protocol a desktop host speaks, so the desktop
app joins it with **Join session** exactly as it joins another person's editor. People can edit
while nobody else is online; the file lives on the server.

```
desigma-server FILE [--password P | --password-env VAR] [--view-only] [--approve-none]
               [--name "Server"] [--key-file PATH] [--link-file PATH] [--direct] [--port PORT]
               [--autosave SECS] [--backups N]
```

On start it prints the join link (`desigma://join/...`) and, with `--link-file`, writes it to a file.

| Option | Meaning |
|---|---|
| `FILE` | A `.desigma` file (created empty if it does not exist), or a `.fig`, `.pen`, Figma REST `.json` or `.svg` to import. An import is saved next to it as `<name>.desigma`; from then on that file is the one served, so restarting with the same command keeps later edits. Delete it to import again. |
| `--password P`, `--password-env VAR` | Guests must give this password. Prefer the environment variable: command lines are visible in `ps`. |
| `--view-only` | Guests can look, not edit. |
| `--approve-none` | Accept everyone who knows the link and password. This is the default and the only mode: there is nobody to click Accept on a headless host. (So use a password.) |
| `--name` | Label in the log lines (default `Server`). |
| `--key-file PATH` | Secret key that keeps the link stable (below). Default `.<name>.desigma-key` next to the document. |
| `--link-file PATH` | Also write the link to this file. |
| `--direct` | No relay and no discovery: LAN, VPN or a fixed public address. Guests must also run with `DESIGMA_COLLAB_NETWORK=direct`. |
| `--port PORT` | Listen on this UDP port instead of a random one. Needed for `--direct` links to be stable, and handy for firewalls. |
| `--autosave SECS` | Save changes at most every SECS seconds (default 10). |
| `--backups N` | Keep N rotating backups, `FILE.bak.1` newest (default 3). A new one is taken at most every 10 minutes, and at start. |

The server saves atomically (temp file, then rename) at the autosave interval and on shutdown.
Ctrl-C, and `SIGTERM` on Unix (`systemctl stop`), save pending edits first. Logging goes to
stdout: joins, leaves, saves, errors. The server has no cursor of its own, but relays the guests'
presence to each other like a desktop host does.

## The stable link

The link holds the server's endpoint id, which comes from a secret key. The key is created on the
first start and kept in the key file (`--key-file`, default `.<name>.desigma-key` beside the
document; mode `0600` on Unix). With the same key file the link is identical after every restart,
so you can pin it in a chat or a wiki. On the internet network the link carries the server's
public key and its relay only; direct addresses are found through the relay, so a changing IP
does not break it. With `--direct` the link contains addresses, so also pin `--port`.

Keep the key file. If you lose it the server gets a new identity and a new link. Do not commit it
or put it in shared backups: whoever has it can impersonate the server.

## Running on a VPS

```
cargo build --release -p desigma-server
sudo install target/release/desigma-server /usr/local/bin/
sudo useradd --system --create-home --home-dir /var/lib/desigma desigma
sudo cp my-design.desigma /var/lib/desigma/ && sudo chown desigma: /var/lib/desigma/my-design.desigma
```

Outbound UDP and HTTPS to the relay are enough; with the default network no inbound port needs
to be open (direct connections are tried through hole punching, otherwise traffic goes through
the relay). To allow direct connections without the relay, open one UDP port with `--port`.

`/etc/desigma/server.env` (mode `0600`) holds the password:

```
DESIGMA_PASSWORD=choose-a-long-one
```

`/etc/systemd/system/desigma-server.service`:

```ini
[Unit]
Description=desigma live collaboration server
After=network-online.target
Wants=network-online.target

[Service]
User=desigma
EnvironmentFile=/etc/desigma/server.env
WorkingDirectory=/var/lib/desigma
ExecStart=/usr/local/bin/desigma-server /var/lib/desigma/my-design.desigma --password-env DESIGMA_PASSWORD --link-file /var/lib/desigma/my-design.link
Restart=on-failure
RestartSec=5
TimeoutStopSec=20
# hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/desigma
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

```
sudo systemctl enable --now desigma-server
journalctl -u desigma-server -f        # the join link is in the first lines
cat /var/lib/desigma/my-design.link
```

## Security notes

* **The link is a secret.** Anyone who has it can try to join. Share it only with people who
  should be able to edit (or view, with `--view-only`).
* **Use a password.** There is no approval step, so the link alone must not be enough. The
  password is checked over the encrypted connection, in constant time, and wrong guesses are
  rate-limited per client.
* Every guest gets the whole document, including embedded images. Guests with edit access can
  change or delete anything; keep the backups (`FILE.bak.N`) and the host's disk snapshots.
* Protect the key file and the password env file (`0600`, owned by the service user).
* All traffic is encrypted end to end (QUIC with the server's key). The relay only sees
  ciphertext.
* On Windows the key file inherits the folder's permissions; keep it in a private folder.

## Not covered yet

* No per-person accounts, approval or `--allow-names` list; the password is shared.
* One document per process (run several instances for several files).
* Fonts: the server has no font system, so a guest's font request is always answered with empty bytes
  (the guest shows its usual missing-font notice). Embedded images are shared, with their digests so
  guests can use their cache.

Guests that lose their connection rejoin as the same person (same peer, same layer ids) with the secret the
server gave them; no approval is involved, and edits made during the outage are merged.
