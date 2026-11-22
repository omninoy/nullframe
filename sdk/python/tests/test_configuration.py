"""Where the token and the port come from, and in what order."""

from __future__ import annotations

import pytest
from fake_nullframe import FakeNullFrame

from nullframe import DEFAULT_HOST, DEFAULT_PORT, NullFrameClient, NullFrameError


def test_arguments_are_used_as_given() -> None:
    client = NullFrameClient(token="from-argument", port=12345, env={})
    assert client.token == "from-argument"
    assert client.port == 12345
    assert client.host == DEFAULT_HOST
    assert client.base_url == "http://127.0.0.1:12345"


def test_the_environment_fills_in_what_was_not_passed() -> None:
    client = NullFrameClient(env={"NULLFRAME_API_TOKEN": "from-env", "NULLFRAME_API_PORT": "13579"})
    assert client.token == "from-env"
    assert client.port == 13579


def test_arguments_win_over_the_environment() -> None:
    client = NullFrameClient(
        token="from-argument",
        port=111,
        env={"NULLFRAME_API_TOKEN": "from-env", "NULLFRAME_API_PORT": "222"},
    )
    assert client.token == "from-argument"
    assert client.port == 111


def test_the_port_falls_back_to_the_app_default() -> None:
    client = NullFrameClient(env={"NULLFRAME_API_TOKEN": "t"})
    assert client.port == DEFAULT_PORT == 10108


def test_a_base_url_overrides_host_and_port() -> None:
    client = NullFrameClient(
        base_url="http://127.0.0.1:9999/nullframe",
        token="t",
        env={"NULLFRAME_API_PORT": "222"},
    )
    assert client.port == 9999
    assert client.base_url == "http://127.0.0.1:9999/nullframe"


def test_a_base_url_prefix_is_kept_on_every_path(fake: FakeNullFrame) -> None:
    with NullFrameClient(
        base_url=f"http://127.0.0.1:{fake.port}/nullframe", token="t", timeout=5.0, env={}
    ) as client:
        client.list_profiles()
    assert fake.last.path == "/nullframe/v1/profiles"


def test_an_unusable_port_in_the_environment_is_reported() -> None:
    with pytest.raises(NullFrameError) as raised:
        NullFrameClient(env={"NULLFRAME_API_TOKEN": "t", "NULLFRAME_API_PORT": "not-a-number"})
    assert "NULLFRAME_API_PORT" in str(raised.value)


def test_an_unsupported_scheme_is_refused() -> None:
    with pytest.raises(NullFrameError):
        NullFrameClient(base_url="ftp://127.0.0.1:9999", token="t", env={})


def test_the_websocket_address_is_built_from_the_same_base() -> None:
    client = NullFrameClient(token="t", port=10108, env={})
    assert (
        client.remote_session_cdp_url("s 1")
        == "ws://127.0.0.1:10108/v1/remote-sessions/s%201/cdp"
    )


def test_a_reopened_client_still_works(fake: FakeNullFrame) -> None:
    """`close()` drops the socket; the next call has to open a new one."""
    with NullFrameClient(token="t", port=fake.port, timeout=5.0, env={}) as client:
        client.list_profiles()
        client.close()
        client.list_profiles()
    assert len(fake.requests) == 2
