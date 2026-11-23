"""One executor for every foreground x background phase.

A cell's phases come from `scenarios`; this module drives them. Probe and
storm differ only in phase parameters (foreground set, client count, issue
policy, stop condition), never in a profile-name branch.
"""

from __future__ import annotations

import itertools
import random
import statistics
import threading
import time
from concurrent import futures

import scenarios
import workload as wl


class ScanSession:
    """Trigger one album scan, poll it, and expose its active interval."""

    def __init__(
        self,
        cfg,
        client,
        logger,
        run_index: int,
        *,
        phase_name: str | None = None,
    ) -> None:
        self.cfg = cfg
        self.client = client
        self.logger = logger
        self.run_index = run_index
        self.out = cfg.out
        self.phase_name = phase_name
        self.active = threading.Event()
        self.started = 0.0
        self.ended = 0.0
        self.wall_ms = 0
        self.state = "unknown"
        self.guard = None
        self.guard_result = None
        self._poller: threading.Thread | None = None

    def __enter__(self):
        self.logger.log(f"=== triggering index scan (run {self.run_index}) ===")
        self.started = time.perf_counter()
        start_ms = int(time.time() * 1000)
        code, body, _ = self.client.post("/post/index/album", {"album": "/"})
        (self.out / "index-trigger.json").write_bytes(body)
        if code != 202:
            raise wl.WorkloadError(f"index trigger returned {code}")
        self.logger.log(f"index trigger=202 start_ms={start_ms}")
        (self.out / "scan-done.marker").unlink(missing_ok=True)
        self.active.set()
        # The poller owns status-last.json; start it and wait for a running
        # snapshot before a guard takes its start snapshot.
        self._poller = threading.Thread(target=self._poll, daemon=True)
        self._poller.start()
        wl._refresh_scan_status(self.cfg, self.client, self.out, self.logger)
        if self.phase_name is not None:
            self.guard = wl.ScanPhase(
                self.out,
                self.phase_name,
                expected_state="running",
                run=self.run_index,
                round_no=self.run_index,
                allow_terminal_end=True,
            )
            self.guard.start()
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        if self._poller is not None:
            self._poller.join(timeout=120)
        if self.guard is not None:
            self.guard_result = self.guard.end()

    def _poll(self) -> None:
        cfg = self.cfg
        out = self.out
        self.logger.log("=== polling /get/index/status ===")
        deadline = time.monotonic() + cfg.scan_timeout_s if cfg.scan_timeout_s > 0 else None
        polls = 0
        last_signature = None
        last_progress = time.monotonic()
        while True:
            try:
                code, body, _ = self.client.get("/get/index/status")
                if code == 200:
                    wl._atomic_write_json_bytes(out, "status-last.json", body)
            except wl.WorkloadError:
                self.logger.log("status poll failed")
            self.state = wl._status_state(out)
            if self.state == "unknown":
                document = {}
            else:
                document = wl._body_json((out / "status-last.json").read_bytes()) or {}
            signature = tuple(
                document.get(key) for key in ("scanned", "matched", "processed", "failed")
            )
            if signature != last_signature:
                last_signature = signature
                last_progress = time.monotonic()
            polls += 1
            if self.state in wl.TERMINAL_STATUS_STATES:
                break
            if polls % 1500 == 0:
                self.logger.log(f"still polling: state={self.state} polls={polls}")
            if (
                cfg.scan_stall_timeout_s > 0
                and time.monotonic() - last_progress >= cfg.scan_stall_timeout_s
            ):
                self.logger.log(
                    f"scan stalled: no counter movement for {cfg.scan_stall_timeout_s:g}s"
                )
                break
            if deadline is not None and time.monotonic() >= deadline:
                break
            time.sleep(cfg.poll_interval_s)

        self.wall_ms = int((time.perf_counter() - self.started) * 1000)
        self.ended = time.perf_counter()
        self.active.clear()
        wl._write_status_final(out, self.run_index)
        (out / "scan-done.marker").write_text(f"state={self.state} wall_ms={self.wall_ms}\n")
        self.logger.log(
            f"scan run {self.run_index}/{self.cfg.scan_repeats}: "
            f"state={self.state} polls={polls} wall_ms={self.wall_ms}"
        )
        wl.emit_event(
            out,
            "scan_state",
            run=self.run_index,
            state=self.state,
            polls=polls,
            wall_ms=self.wall_ms,
        )


def _rows(cfg, client, logger=None) -> list:
    lines = wl.build_probe_list(cfg, client) or []
    rows = [tuple(line.split("\t", 1)) for line in lines]
    if not rows:
        lines = wl._wait_for_rows(cfg, client, logger, None, min_rows=1)
        rows = [tuple(line.split("\t", 1)) for line in lines]
    return rows


def _issue_request(cfg, client, access: str, rows: list, timestamp) -> tuple:
    """One timed request; returns (elapsed_s, code)."""
    try:
        if access == "prefetch":
            code, _body, elapsed = client.post("/get/prefetch", {})
        elif access == "edit":
            index = random.randrange(len(rows)) if rows else 0
            code, _body, elapsed = client.put(
                "/put/edit_rating",
                {"indexArray": [index], "timestamp": timestamp, "rating": 3},
            )
        elif access == "thumb":
            hash_value, token = random.choice(rows)
            code, _body, elapsed = client.get(wl._compressed_path(hash_value), bearer=token)
        else:
            raise wl.WorkloadError(f"unknown request foreground: {access}")
    except wl.WorkloadError:
        return 0.0, 0
    return float(elapsed), code


def drive_rounds_scan(cfg, client, logger, session: ScanSession, target: int, warm: bool) -> int:
    """Issue probe rounds until the scan settles; return valid-round count."""
    transcript = wl.ProbeTranscript(logger, cfg.out / "probe-phase.log")
    rows_cache: list = []
    valid = 0
    rounds = 0
    warmed = not warm
    try:
        while session.active.is_set():
            state = wl._status_state(cfg.out)
            if state in wl.TERMINAL_STATUS_STATES or (cfg.out / "scan-done.marker").is_file():
                break
            if not rows_cache:
                rows_cache = wl.build_probe_list(cfg, client) or []
            if not rows_cache:
                time.sleep(cfg.probe_interval_s)
                continue
            if not warmed and cfg.scan_warmup > 0:
                warmed = True
                try:
                    result = wl.warmup_scan(cfg, client, cfg.out, session.run_index, rows_cache)
                    logger.log(
                        f"warmup: valid={result['valid']} reason={result['reason']} "
                        f"wall_ms={result['wall_ms']}"
                    )
                except Exception as err:  # noqa: BLE001 - warmup must not kill the scan
                    logger.log(f"warn: warmup failed: {err}")
            rounds += 1
            guard = wl.ScanPhase(
                cfg.out,
                "probe",
                expected_state="running",
                run=session.run_index,
                round_no=rounds,
                snapshot=lambda: wl._status_snapshot(cfg.out),
            )
            guard.start()
            ok = False
            try:
                ok = bool(
                    wl.probe_round(
                        cfg,
                        client,
                        logger,
                        transcript,
                        rounds,
                        len(rows_cache),
                        state,
                        rows_cache,
                    )
                )
            except Exception as err:  # noqa: BLE001 - a probe must not kill the scan
                transcript.log(f"probe round {rounds} failed; continuing: {err}")
            result = guard.end()
            if ok and result.get("valid"):
                valid += 1
                if target and valid >= target:
                    break
            time.sleep(cfg.probe_interval_s)
    finally:
        transcript.close()
    logger.log(f"probe phase: rounds={rounds} valid={valid}")
    return valid


def drive_rounds_idle(cfg, client, logger, phase) -> None:
    """Time each request foreground at rest; scroll once per phase."""
    rows = _rows(cfg, client, logger)
    timestamp = wl._prefetch_timestamp(cfg.out)
    if timestamp is None:
        code, body, _ = client.post("/get/prefetch", {})
        if code != 200:
            raise wl.WorkloadError("idle probes: prefetch failed")
        (cfg.out / "prefetch.json").write_bytes(body)
        timestamp = wl._prefetch_timestamp(cfg.out)
    for access in ("prefetch", "thumb", "edit"):
        for _ in range(cfg.warmup):
            elapsed, code = _issue_request(cfg, client, access, rows, timestamp)
            wl.emit_event(
                cfg.out,
                "idle_sample",
                access=access,
                latency_ms=elapsed * 1000.0,
                code=code,
                phase="warmup",
            )
    if "scroll" in phase.foregrounds:
        token = wl._prefetch_token(cfg.out)
        if timestamp is not None and token is not None:
            result = wl.scroll_probe(
                cfg,
                client,
                timestamp,
                token,
                total_rows=len(rows) or None,
            )
            wl.emit_event(
                cfg.out,
                "scroll_probe",
                phase="post",
                background="idle",
                scrollbar_ms=[v * 1000.0 for v in result["scrollbar"]],
                rows_ms=[v * 1000.0 for v in result["rows"]],
                data_ms=[v * 1000.0 for v in result["data"]],
            )
    for _ in range(phase.target):
        for access in phase.foregrounds:
            if access == "scroll":
                continue
            elapsed, code = _issue_request(cfg, client, access, rows, timestamp)
            wl.emit_event(
                cfg.out,
                "idle_sample",
                access=access,
                latency_ms=elapsed * 1000.0,
                code=code,
                phase="measured",
            )


def drive_stream(cfg, client, logger, phase, session: ScanSession | None, run_index: int) -> dict:
    """N concurrent clients issuing one access until the stop condition."""
    access = phase.foregrounds[0]
    rows = _rows(cfg, client, logger)
    timestamp = wl._prefetch_timestamp(cfg.out)
    target = phase.target if phase.stop == "ops" else 0
    results: list = []
    lock = threading.Lock()
    counter = itertools.count()
    active_since = [None]
    started_all = time.perf_counter()

    def worker(client_index: int) -> None:
        while True:
            if session is not None and not session.active.is_set():
                if active_since[0] is None:
                    active_since[0] = time.perf_counter() - started_all
                return
            op_index = next(counter)
            if target and op_index >= target:
                if active_since[0] is None:
                    active_since[0] = time.perf_counter() - started_all
                return
            elapsed, code = _issue_request(cfg, client, access, rows, timestamp)
            with lock:
                results.append((op_index, client_index, elapsed, code))
            wl.emit_event(
                cfg.out,
                "storm_sample",
                profile=cfg.workload,
                client=client_index,
                op=op_index,
                latency_ms=elapsed * 1000.0,
                code=code,
            )

    with futures.ThreadPoolExecutor(max_workers=phase.clients) as pool:
        submitted = [pool.submit(worker, index) for index in range(phase.clients)]
        for future in futures.as_completed(submitted):
            future.result()

    wall = time.perf_counter() - started_all
    active_wall = active_since[0] if active_since[0] is not None else wall
    latencies = sorted(item[2] for item in results)
    failed = sum(1 for item in results if item[3] != 200)
    summary = {
        "profile": cfg.workload,
        "kind": access,
        "done": len(results),
        "total": target or wl.storm_minimum_total(phase.clients, cfg.storm_ops, True),
        "median": statistics.median(latencies) if latencies else 0.0,
        "p95": wl._percentile(latencies, 95),
        "max": max(latencies) if latencies else 0.0,
        "failed": failed,
        "wall": wall,
        "active_wall": active_wall,
        "coverage_pct": None,
    }
    logger.log(
        f"storm: profile={cfg.workload} ops={summary['done']}/{summary['total']} "
        f"failed={failed} wall={wall:.1f}s kind={access} active_wall={active_wall:.1f}s"
    )
    return summary


def _emit_storm_summary(cfg, summary: dict) -> None:
    wl.emit_event(
        cfg.out,
        "storm_summary",
        profile=cfg.workload,
        kind=summary["kind"],
        done=summary["done"],
        total=summary["total"],
        median=summary["median"],
        p95=summary["p95"],
        max=summary["max"],
        failed=summary["failed"],
        wall=summary["wall"],
        active_wall=summary["active_wall"],
        coverage_pct=summary["coverage_pct"],
        median_ms=summary["median"] * 1000.0,
        p95_ms=summary["p95"] * 1000.0,
        max_ms=summary["max"] * 1000.0,
        wall_ms=summary["wall"] * 1000.0,
        active_wall_ms=summary["active_wall"] * 1000.0,
    )


def drive_files(cfg, client, logger, phase, session: ScanSession | None) -> None:
    """Serial upload submissions; visibility is polled per file at rest.

    Under a scan background the submissions stay serial but the visibility
    wait is batched after them: waiting for each row before the next submit
    would consume the whole scan with the first file and leave one sample.
    """
    sources = wl._select_upload_sources(cfg)[: phase.target]

    def submit(source):
        active = session.active.is_set() if session else False
        status, body, elapsed = client.upload(
            "/post/upload", source, timeout=cfg.upload_post_timeout_s
        )
        if status != 200:
            raise wl.WorkloadError(f"upload failed for {source.name}: HTTP {status}")
        doc = wl._body_json(body)
        files = doc.get("files") if isinstance(doc, dict) else None
        stored_name = (
            files[0]
            if isinstance(files, list) and files and isinstance(files[0], str)
            else source.name
        )
        return stored_name, int(elapsed * 1000), active

    def wait_visible(stored_name, started):
        deadline = time.monotonic() + cfg.upload_timeout_s
        while time.monotonic() < deadline:
            if wl._uploaded_row_visible(cfg, client, stored_name):
                return int((time.perf_counter() - started) * 1000)
            time.sleep(cfg.poll_interval_s)
        return None

    submits: list = []
    visibles: list = []
    if session is None:
        for source in sources:
            started = time.perf_counter()
            stored_name, submit_ms, active = submit(source)
            visible_ms = wait_visible(stored_name, started)
            if visible_ms is None:
                raise wl.WorkloadError(
                    f"upload verification FAILED: file={stored_name} never became visible"
                )
            logger.log(f"upload file={stored_name} submit_ms={submit_ms} visible_ms={visible_ms}")
            wl.emit_event(
                cfg.out,
                "upload_file",
                file=stored_name,
                submit_ms=submit_ms,
                visible_ms=visible_ms,
                scan_active_at_submit=active,
                scan_active_at_visible=False,
            )
            submits.append(submit_ms)
            visibles.append(visible_ms)
        _log_upload_summary(cfg, logger, submits, visibles)
        return

    pending = []
    for source in sources:
        started = time.perf_counter()
        stored_name, submit_ms, active = submit(source)
        pending.append((stored_name, submit_ms, active, started))
        logger.log(f"upload file={stored_name} submit_ms={submit_ms} queued")
    for stored_name, submit_ms, active, started in pending:
        visible_ms = wait_visible(stored_name, started)
        if visible_ms is None:
            raise wl.WorkloadError(
                f"upload verification FAILED: file={stored_name} never became visible"
            )
        logger.log(f"upload file={stored_name} submit_ms={submit_ms} visible_ms={visible_ms}")
        wl.emit_event(
            cfg.out,
            "upload_file",
            file=stored_name,
            submit_ms=submit_ms,
            visible_ms=visible_ms,
            scan_active_at_submit=active,
            scan_active_at_visible=session.active.is_set(),
        )
        submits.append(submit_ms)
        visibles.append(visible_ms)
    _log_upload_summary(cfg, logger, submits, visibles)


def _log_upload_summary(cfg, logger, submits: list, visibles: list) -> None:
    if not submits:
        return
    submit_p50 = int(statistics.median(submits))
    visible_p50 = int(statistics.median(visibles)) if visibles else 0
    logger.log(f"upload submit_p50={submit_p50} visible_p50={visible_p50}")
    wl.emit_event(
        cfg.out,
        "upload_summary",
        submit_p50=submit_p50,
        visible_p50=visible_p50,
    )


def execute_cell(cfg, client, logger, spec: scenarios.CellSpec) -> None:
    """Run every phase of one cell spec."""
    walls: list = []
    warm = True
    for phase in spec.phases:
        if phase.background == "scan" and phase.issue == "rounds":
            valid = 0
            scans = 0
            while valid < spec.probe_round_target and scans < cfg.scan_repeats:
                scans += 1
                with ScanSession(cfg, client, logger, scans) as session:
                    valid += drive_rounds_scan(
                        cfg, client, logger, session, spec.probe_round_target - valid, warm
                    )
                warm = False
                walls.append(session.wall_ms)
            if valid < spec.probe_round_target:
                raise wl.WorkloadError(
                    f"probe rounds: {valid} valid after {scans} scan(s), "
                    f"budget {spec.probe_round_target}"
                )
            continue
        if phase.background == "scan" and phase.issue == "stream":
            with ScanSession(cfg, client, logger, 1, phase_name="storm") as session:
                summary = drive_stream(cfg, client, logger, phase, session, 1)
            if session.guard_result is not None and not session.guard_result.get("valid", False):
                raise wl.WorkloadError(f"storm guard invalid: {session.guard_result.get('reason')}")
            wl.check_storm_coverage(
                summary, session.started, session.ended, cfg.storm_min_coverage_pct
            )
            _emit_storm_summary(cfg, summary)
            if phase.foregrounds[0] == "edit":
                wl.verify_storm_sample(cfg, client, logger, cfg.out, 1)
            walls.append(session.wall_ms)
            continue
        if phase.background == "scan" and phase.issue == "files":
            with ScanSession(cfg, client, logger, 1) as session:
                drive_files(cfg, client, logger, phase, session)
            walls.append(session.wall_ms)
            continue
        if phase.setup_scan:
            with ScanSession(cfg, client, logger, 1) as session:
                pass
            walls.append(session.wall_ms)
        if phase.issue == "stream":
            guard = wl.ScanPhase(cfg.out, "storm", expected_state="completed")
            guard.start()
            try:
                summary = drive_stream(cfg, client, logger, phase, None, 1)
            finally:
                result = guard.end()
            if not result.get("valid", False):
                raise wl.WorkloadError(f"idle storm guard invalid: {result.get('reason')}")
            _emit_storm_summary(cfg, summary)
            if phase.foregrounds[0] == "edit":
                wl.verify_storm_sample(cfg, client, logger, cfg.out, 1)
            continue
        if phase.issue == "files":
            drive_files(cfg, client, logger, phase, None)
            continue
        drive_rounds_idle(cfg, client, logger, phase)
    if walls:
        cfg.scan_wall_ms = statistics.median(walls)
        cfg.scan_runs = len(walls)
        cfg.scan_walls_ms = list(walls)
