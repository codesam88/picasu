## Build from Source (Linux Version)

Follow these steps to set up and run the Picasu app on Linux-based systems.

### 1. Clone the Repository

```bash
git clone <your-picasu-repo-url>
```

This will create a folder called `./picasu`.

---

### 2. Install Dependencies

**Development and tests run against pinned versions.** The external
binaries the test suite needs are installed from a pinned release with a
checked digest, so a test run is reproducible and the version the tests
exercise is the version the release workflow packages:

```bash
just setup-dev    # installs every pinned tool, including ffmpeg and ExifTool
```

- **ffmpeg**: the backend shells out to `ffmpeg` (thumbnail, compression) and
  `ffprobe` (dimensions, container metadata) for video files. Install the
  pinned static build without root:

  ```bash
  just install-ffmpeg
  ```

- **ExifTool**: snapfab (`utils/snapfab`), the fixture generator behind the
  test suite, writes fixture metadata with `exiftool`. The backend itself
  reads and writes metadata in-process (kamadak-exif + xmpkit) and never
  spawns it, so **running the app does not need ExifTool** — only
  `cargo test -p snapfab` (part of `just test`) does. Install the pinned
  release without root:

  ```bash
  just install-exiftool
  ```

  A distribution's package works too (Debian/Ubuntu:
  `sudo apt install libimage-exiftool-perl`), but then the tested ExifTool
  and the developer's can differ.

**A deployment does not need either of these.** The server needs ffmpeg at
runtime and nothing else; see [Deploying a release](#deploying-a-release)
for the recommended way to get it.

- **Rust**: Install Rust using the official installer:

  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source $HOME/.cargo/env
  ```

- **npm (Node.js)**: Install Node.js (with npm). For Ubuntu, use APT:

  ```bash
  sudo apt install -y nodejs npm
  ```

  For other Linux distributions, use the appropriate package manager (e.g., `dnf`, `yum`, `pacman`) and find the corresponding package name for installation.

---

### 3. Build the Frontend

In the `frontend` directory, run:

```bash
npm run build
```

---

### 4. Run the Application

Navigate to the `backend` directory and run the following command to start the app:

```bash
cargo run --release
```

You can now access the app via [http://127.0.0.1:5673](http://127.0.0.1:5673) or [http://127.0.0.1](http://127.0.0.1):\<your_port> if you configured a custom port in Rocket.toml.

---

## Deploying a release

A release publishes two packages:

| artifact                             | contains                              | use it when                                   |
| ------------------------------------ | ------------------------------------- | --------------------------------------------- |
| `picasu-linux-<version>.tar.gz`      | the server                            | you install ffmpeg from your own distribution |
| `picasu-linux-<version>-bundled.tgz` | the server + the pinned static ffmpeg | you want the exact build the release tested   |

**The plain package is the recommended one.** The only runtime dependency
is ffmpeg, and your distribution ships a version it supports and updates:

```bash
sudo apt update && sudo apt install -y ffmpeg   # Debian/Ubuntu
# dnf install ffmpeg / pacman -S ffmpeg / …     # other distributions
```

Unpack the server and run it:

```bash
tar -xzf picasu-linux-<version>.tar.gz
cd picasu-linux
./picasu
```

The bundled package is the alternative for a host without a package manager
for ffmpeg, or when the exact tested build matters more than the
distribution's. It carries the pinned static ffmpeg and ffprobe in `bin/`,
so the server finds them next to itself:

```bash
tar -xzf picasu-linux-<version>-bundled.tgz
cd picasu-linux
PATH="$PWD/bin:$PATH" ./picasu
```

The bundled ffmpeg is a fixed version: it does not move with your
distribution's security updates, which is the reason the plain package is
the default.

---

## Update

### 1. Pull the Latest Changes from the Repository

Navigate to the project directory and pull the latest updates:

```bash
git pull
```

### 2. Rebuild

### Rebuild the Frontend

1. Navigate to the `frontend` directory:

   ```bash
   cd ./picasu/frontend
   ```

2. Build the frontend:

   ```bash
   npm run build
   ```

### Rebuild the Backend

1. Navigate to the `backend` directory:

   ```bash
   cd ./picasu/backend
   ```

2. Build and run the backend using Cargo:

   ```bash
   cargo run --release
   ```

After following these steps, your Picasu app will be updated to the latest version.
