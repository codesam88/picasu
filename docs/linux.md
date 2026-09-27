## Build from Source (Linux Version)

Follow these steps to set up and run the Picasu app on Linux-based systems.

### 1. Clone the Repository

```bash
git clone <your-picasu-repo-url>
```

This will create a folder called `./picasu`.

---

### 2. Install Dependencies

Make sure the following software is installed on your system:

- **ffmpeg / ffprobe**: required for the video features (dimensions, metadata,
  thumbnails). Install via your system's package manager. For Ubuntu, use APT:

  ```bash
  sudo apt update && sudo apt install -y ffmpeg
  ```

  For other Linux distributions, use the appropriate package manager (e.g., `dnf`, `yum`, `pacman`) and find the corresponding package name for installation.

- **ExifTool**: the image metadata engine, also an external binary, and required
  for image metadata. For Ubuntu, use APT:

  ```bash
  sudo apt install -y libimage-exiftool-perl
  ```

  Or, without root, install the pinned pure-Perl distribution from the repository
  root with `just install-exiftool` (unpacks into `~/.local` and symlinks
  `~/.local/bin/exiftool`).

Both are prerequisites of a running Picasu, not optional extras: without
`ffprobe` a video has no dimensions or metadata, and ExifTool is the only image
metadata reader, so without it every image reports no metadata at all. Neither
tool is bundled with Picasu — a release tarball from the GitHub releases page
contains the `picasu` binary alone, so install both the same way on a downloaded
build as when building from source.

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
