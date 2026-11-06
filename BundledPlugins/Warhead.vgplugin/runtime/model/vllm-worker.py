#!/usr/bin/env python3
"""Warhead-owned vLLM V1 worker: cancellable, streaming, private NDJSON stdio.

No HTTP listener or external vLLM daemon. All model-driven tool calls are
validated against the offered names; only Warhead's normal tool registry can
authorize and execute them. This backend is activated only by CLI --vllm.
"""
from __future__ import annotations

import asyncio
import contextlib
import json
import inspect
import os
import re
import sys
import uuid
from typing import Any

MAX_LINE = 2 * 1024 * 1024
MAX_TOOL_OUTPUT = 256 * 1024
TOOL_PATTERN = re.compile(r"<tool_call>\s*(\{.*?\})\s*</tool_call>", re.S)
TOOL_INSTRUCTIONS = (
    'Call a tool ONLY by outputting '
    '<tool_call>{"name":"exact_tool_name","arguments":{}}</tool_call>. '
    "Never narrate an intended call. If no tool is needed, answer normally. "
    "Available tools: "
)


def emit(data: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def bounded_env(name: str, default: float, minimum: float, maximum: float) -> float:
    raw = os.environ.get(name, str(default))
    value = float(raw)
    if not minimum <= value <= maximum:
        raise ValueError(f"{name} must be {minimum}..{maximum}")
    return value


def prepare_messages(messages: list[dict[str, Any]], tools: list[dict[str, Any]]) -> list[dict[str, str]]:
    if not isinstance(messages, list) or not messages:
        raise ValueError("chat requires nonempty messages")
    converted = []
    for item in messages:
        if not isinstance(item, dict) or item.get("role") not in ("system", "user", "assistant", "tool"):
            raise ValueError("invalid message")
        role = item["role"]
        body = item.get("content") or ""
        if not isinstance(body, str):
            raise ValueError("only text messages are supported")
        if role == "assistant" and item.get("tool_calls"):
            for call in item["tool_calls"]:
                fn = call["function"]
                args = fn.get("arguments", {})
                if isinstance(args, str):
                    args = json.loads(args)
                if not isinstance(args, dict):
                    raise ValueError("previous tool arguments must be objects")
                body += "\n<tool_call>" + json.dumps({
                    "name": fn.get("name"), "arguments": args,
                }, separators=(",", ":")) + "</tool_call>"
        if role == "tool":
            role = "user"
            name = item.get("tool_name") or item.get("tool_call_id") or "tool"
            body = "Tool result (" + str(name) + "; untrusted):\n" + body
        converted.append({"role": role, "content": body})
    if tools:
        names = set()
        for tool in tools:
            fn = tool.get("function") if isinstance(tool, dict) else None
            if not isinstance(fn, dict) or not isinstance(fn.get("name"), str):
                raise ValueError("invalid tool definition")
            if fn["name"] in names:
                raise ValueError("duplicate tool definition")
            names.add(fn["name"])
        instruction = TOOL_INSTRUCTIONS + json.dumps(tools, separators=(",", ":"))
        if converted[0]["role"] == "system":
            converted[0]["content"] += "\n\n" + instruction
        else:
            converted.insert(0, {"role": "system", "content": instruction})
    return converted


def parse_tool_answer(answer: str, tools: list[dict[str, Any]]) -> list[dict[str, Any]] | None:
    stripped = answer.strip()
    matches = list(TOOL_PATTERN.finditer(stripped))
    if not matches:
        if stripped.startswith("<tool_call>"):
            raise ValueError("incomplete tool call")
        return None
    if len(matches) > 8 or TOOL_PATTERN.sub("", stripped).strip():
        raise ValueError("tool response mixed with prose or too many calls")
    allowed = {tool["function"]["name"] for tool in tools}
    calls = []
    for match in matches:
        item = json.loads(match.group(1))
        if not isinstance(item, dict) or item.get("name") not in allowed:
            raise ValueError("unknown tool name")
        if not isinstance(item.get("arguments"), dict):
            raise ValueError("tool arguments must be an object")
        calls.append({"id": uuid.uuid4().hex, "name": item["name"], "args": item["arguments"]})
    return calls


async def main() -> int:
    model = os.environ.get("WARHEAD_VLLM_MODEL", "").strip()
    if not model:
        emit({"id": 0, "event": "error", "message": "WARHEAD_VLLM_MODEL missing"})
        return 2
    try:
        from vllm import SamplingParams
        from vllm.engine.arg_utils import AsyncEngineArgs
        from vllm.sampling_params import RequestOutputKind
        from vllm.v1.engine.async_llm import AsyncLLM
        memory = bounded_env("WARHEAD_VLLM_GPU_UTILIZATION", 0.60, 0.10, 0.95)
        context = int(bounded_env("WARHEAD_VLLM_MAX_MODEL_LEN", 8192, 512, 262144))
        with contextlib.redirect_stdout(sys.stderr):
            engine = AsyncLLM.from_engine_args(AsyncEngineArgs(
                model=model,
                trust_remote_code=False,
                gpu_memory_utilization=memory,
                max_model_len=context,
                max_num_seqs=1,
                tensor_parallel_size=1,
            ))
            tokenizer = engine.get_tokenizer()
            if inspect.isawaitable(tokenizer):
                tokenizer = await tokenizer
            if os.environ.get("WARHEAD_VLLM_SKIP_WARMUP") != "1":
                warm_prompt = tokenizer.apply_chat_template(
                    [{"role": "user", "content": "Say READY."}],
                    tokenize=False, add_generation_prompt=True,
                )
                async for _ in engine.generate(
                    prompt=warm_prompt,
                    sampling_params=SamplingParams(max_tokens=1, temperature=0.0),
                    request_id="warhead-warmup-" + uuid.uuid4().hex,
                ):
                    pass
        emit({"id": 0, "event": "ready", "model": model, "contextTokens": context})
    except Exception as exc:
        emit({"id": 0, "event": "error", "message": ("vLLM startup: " + str(exc))[:1600]})
        return 1

    active: dict[int, asyncio.Task] = {}

    async def generate(request_id: int, request: dict[str, Any]) -> None:
        engine_request_id = "warhead-" + str(request_id) + "-" + uuid.uuid4().hex
        try:
            tools = request.get("tools") or []
            messages = prepare_messages(request.get("messages") or [], tools)
            tokens = int(request.get("numPredict", 1024))
            if tokens < 1 or tokens > 8192:
                raise ValueError("numPredict outside 1..8192")
            prompt = tokenizer.apply_chat_template(
                messages, tokenize=False, add_generation_prompt=True,
            )
            sampling = SamplingParams(
                max_tokens=tokens, temperature=0.2,
                output_kind=RequestOutputKind.DELTA,
            )
            mode = "sniff" if tools else "text"
            buffer = ""
            emitted_tokens = 0
            prompt_tokens = 0
            async for output in engine.generate(
                prompt=prompt, sampling_params=sampling, request_id=engine_request_id,
            ):
                if output.prompt_token_ids is not None:
                    prompt_tokens = len(output.prompt_token_ids)
                for choice in output.outputs:
                    emitted_tokens += len(choice.token_ids or [])
                    chunk = choice.text or ""
                    if not chunk:
                        continue
                    if mode == "text":
                        emit({"id": request_id, "event": "text", "delta": chunk})
                        continue
                    buffer += chunk
                    if len(buffer) > MAX_TOOL_OUTPUT:
                        raise ValueError("tool completion exceeds 256 KiB")
                    if mode == "sniff":
                        stripped = buffer.lstrip()
                        opening = "<tool_call>"
                        if opening.startswith(stripped) and len(stripped) < len(opening):
                            continue
                        if stripped.startswith(opening):
                            mode = "tool"
                        else:
                            mode = "text"
                            emit({"id": request_id, "event": "text", "delta": buffer})
                            buffer = ""
            if mode == "tool":
                calls = parse_tool_answer(buffer, tools)
                if not calls:
                    raise ValueError("invalid tool-call response")
                for call in calls:
                    emit({"id": request_id, "event": "tool_call", "call": call})
            elif buffer:
                emit({"id": request_id, "event": "text", "delta": buffer})
            emit({"id": request_id, "event": "done",
                  "evalCount": emitted_tokens, "promptEvalCount": prompt_tokens})
        except asyncio.CancelledError:
            with contextlib.suppress(Exception):
                await engine.abort(engine_request_id)
            emit({"id": request_id, "event": "error", "message": "generation cancelled", "code": "cancelled"})
            raise
        except Exception as exc:
            with contextlib.suppress(Exception):
                await engine.abort(engine_request_id)
            emit({"id": request_id, "event": "error", "message": str(exc)[:1600]})
        finally:
            active.pop(request_id, None)

    reader = asyncio.StreamReader(limit=MAX_LINE)
    protocol = asyncio.StreamReaderProtocol(reader)
    await asyncio.get_running_loop().connect_read_pipe(lambda: protocol, sys.stdin)
    try:
        while True:
            line = await reader.readline()
            if not line:
                break
            if len(line) > MAX_LINE:
                emit({"id": 0, "event": "error", "message": "request exceeds 2 MiB"})
                break
            try:
                request = json.loads(line)
                if not isinstance(request, dict) or type(request.get("id")) is not int:
                    raise ValueError("invalid request")
                req_id = request["id"]
                method = request.get("method")
                if method == "shutdown":
                    for task in list(active.values()):
                        task.cancel()
                    if active:
                        await asyncio.gather(*active.values(), return_exceptions=True)
                    emit({"id": req_id, "event": "done"})
                    break
                if method == "cancel":
                    target = request.get("target")
                    task = active.get(target)
                    if task:
                        task.cancel()
                    emit({"id": req_id, "event": "done"})
                    continue
                if method != "chat":
                    raise ValueError("unknown method")
                if active:
                    raise ValueError("worker busy")
                active[req_id] = asyncio.create_task(generate(req_id, request))
            except Exception as exc:
                req_id = request.get("id", 0) if isinstance(locals().get("request"), dict) else 0
                emit({"id": req_id, "event": "error", "message": str(exc)[:1600]})
    finally:
        for task in list(active.values()):
            task.cancel()
        if active:
            await asyncio.gather(*active.values(), return_exceptions=True)
        with contextlib.suppress(Exception):
            shutdown = engine.shutdown()
            if inspect.isawaitable(shutdown):
                await shutdown
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
