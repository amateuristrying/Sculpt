import io
import json
from pathlib import Path
import pytest

from sculpt_backend.protocol import (
    PROTOCOL_VERSION,
    VALID_STAGES,
    emit,
    emit_accepted,
    emit_cancelled,
    emit_error,
    emit_progress,
    emit_result,
    emit_stage,
    emit_warning,
    format_envelope,
    parse_envelope,
    progress,
    validate_output_path,
)


def test_format_and_parse_all_event_types():
    job_id = "test-job-123"

    # 1. accepted
    buf = io.StringIO()
    emit_accepted(job_id=job_id, message="Request accepted", stream=buf)
    parsed = parse_envelope(buf.getvalue())
    assert parsed["protocol"] == 1
    assert parsed["type"] == "accepted"
    assert parsed["job_id"] == job_id
    assert parsed["message"] == "Request accepted"

    # 2. stage
    for stage in VALID_STAGES:
        buf = io.StringIO()
        emit_stage(stage=stage, job_id=job_id, message=f"Entering {stage}", stream=buf)
        parsed = parse_envelope(buf.getvalue())
        assert parsed["type"] == "stage"
        assert parsed["stage"] == stage
        assert parsed["message"] == f"Entering {stage}"

    # 3. progress
    buf = io.StringIO()
    emit_progress(stage="geometry", progress=45.5, message="Reconstructing surface", job_id=job_id, stream=buf)
    parsed = parse_envelope(buf.getvalue())
    assert parsed["type"] == "progress"
    assert parsed["stage"] == "geometry"
    assert parsed["progress"] == 45.5
    assert parsed["message"] == "Reconstructing surface"

    # 4. warning
    buf = io.StringIO()
    emit_warning(message="Low VRAM warning", code="LOW_VRAM", job_id=job_id, stream=buf)
    parsed = parse_envelope(buf.getvalue())
    assert parsed["type"] == "warning"
    assert parsed["code"] == "LOW_VRAM"
    assert parsed["message"] == "Low VRAM warning"

    # 5. result
    buf = io.StringIO()
    emit_result(
        metrics={"faces": 5000, "vertices": 2500},
        artifacts=["mesh.glb", "preview.png"],
        duration_ms=4500,
        peak_memory_mb=768.2,
        job_id=job_id,
        stream=buf,
    )
    parsed = parse_envelope(buf.getvalue())
    assert parsed["type"] == "result"
    assert parsed["metrics"] == {"faces": 5000, "vertices": 2500}
    assert parsed["artifacts"] == ["mesh.glb", "preview.png"]
    assert parsed["duration_ms"] == 4500
    assert parsed["peak_memory_mb"] == 768.2

    # 6. error
    buf = io.StringIO()
    emit_error(message="CUDA Out of Memory", code="OOM", job_id=job_id, stream=buf)
    parsed = parse_envelope(buf.getvalue())
    assert parsed["type"] == "error"
    assert parsed["code"] == "OOM"
    assert parsed["message"] == "CUDA Out of Memory"

    # 7. cancelled
    buf = io.StringIO()
    emit_cancelled(message="Job cancelled by user", job_id=job_id, stream=buf)
    parsed = parse_envelope(buf.getvalue())
    assert parsed["type"] == "cancelled"
    assert parsed["message"] == "Job cancelled by user"


def test_reject_missing_or_unsupported_protocol_version():
    with pytest.raises(ValueError, match="Missing protocol version"):
        parse_envelope('{"type": "accepted"}')

    with pytest.raises(ValueError, match="Unsupported protocol version"):
        parse_envelope('{"protocol": 2, "type": "accepted"}')

    with pytest.raises(ValueError, match="Unsupported protocol version"):
        parse_envelope('{"protocol": 0, "type": "accepted"}')


def test_alias_support():
    # protocol_version and event aliases
    raw = '{"protocol_version": 1, "event": "stage", "stage": "surface"}'
    parsed = parse_envelope(raw)
    assert parsed["protocol_version"] == 1
    assert parsed["event"] == "surface" or parsed.get("stage") == "surface"


def test_reject_missing_or_unknown_event_type():
    with pytest.raises(ValueError, match="Missing event type"):
        parse_envelope('{"protocol": 1}')

    with pytest.raises(ValueError, match="Unknown protocol event type"):
        parse_envelope('{"protocol": 1, "type": "telemetry_ping"}')


def test_stage_validation():
    with pytest.raises(ValueError, match="Unknown inference stage"):
        emit_stage("baking_texture")

    with pytest.raises(ValueError, match="Invalid or missing stage"):
        parse_envelope('{"protocol": 1, "type": "stage", "stage": "invalid_stage"}')


def test_progress_validation():
    with pytest.raises(ValueError, match="Unknown inference stage"):
        emit_progress("invalid_stage", 50.0, "processing")

    with pytest.raises(ValueError, match="Missing or invalid numeric progress"):
        parse_envelope('{"protocol": 1, "type": "progress", "stage": "loading", "progress": "fifty"}')


def test_path_safety_validation(tmp_path):
    workspace = tmp_path / "workspace"
    workspace.mkdir()

    # Safe relative path
    safe_rel = validate_output_path("mesh.glb", workspace)
    assert safe_rel == (workspace / "mesh.glb").resolve()

    # Safe subpath
    (workspace / "sub").mkdir()
    safe_sub = validate_output_path("sub/preview.png", workspace)
    assert safe_sub == (workspace / "sub" / "preview.png").resolve()

    # Safe absolute path
    safe_abs_path = workspace / "asset.glb"
    safe_abs = validate_output_path(safe_abs_path, workspace)
    assert safe_abs == safe_abs_path.resolve()

    # Traversal relative with '..'
    with pytest.raises(ValueError, match="Path traversal forbidden"):
        validate_output_path("../../etc/passwd", workspace)

    with pytest.raises(ValueError, match="Path traversal forbidden"):
        validate_output_path("sub/../../escaped.glb", workspace)

    # Absolute path outside workspace
    outside = tmp_path / "outside"
    outside.mkdir()
    outside_file = outside / "secret.txt"
    with pytest.raises(ValueError, match="escapes job workspace"):
        validate_output_path(outside_file, workspace)


def test_backward_compatibility():
    # Legacy emit function
    buf = io.StringIO()
    emit("progress", stage="analyzing", progress=12.0, message="Analyzing", stream=buf)
    parsed = parse_envelope(buf.getvalue())
    assert parsed["protocol"] == 1
    assert parsed["type"] == "progress"
    assert parsed["stage"] == "analyzing"
    assert parsed["progress"] == 12.0

    # Legacy progress helper
    buf2 = io.StringIO()
    progress("loading", 25.0, "Loading model", stream=buf2)
    parsed2 = parse_envelope(buf2.getvalue())
    assert parsed2["stage"] == "loading"
    assert parsed2["progress"] == 25.0
