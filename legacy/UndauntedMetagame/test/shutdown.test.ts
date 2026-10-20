import { RemoveTestDb } from "./setup";
import "./authenv";
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";

after(() => RemoveTestDb(() => {}));

async function FreePort(): Promise<number> {
    const Server = net.createServer();
    Server.listen(0, "127.0.0.1");
    await once(Server, "listening");
    const Port = (Server.address() as net.AddressInfo).port;
    await new Promise<void>(Resolve => Server.close(() => Resolve()));
    return Port;
}

for(const Mode of ["early", "chat-pending", "http-pending", "ready", "chat-disabled"]){
    test(`Metagame shutdown is idempotent and releases listeners (${Mode})`, { timeout: 20_000 }, async () => {
        const Dir = fs.mkdtempSync(path.join(os.tmpdir(), "dr-shutdown-"));
        const Port = await FreePort();
        const ChatPort = await FreePort();
        // Use the actual compiled service, migrations and chat. Gates only delay startup
        // boundaries so shutdown races can be exercised without depending on machine speed.
        const Script = `
            const assert = require("node:assert/strict");
            const net = require("node:net");
            const mode = ${JSON.stringify(Mode)};
            let release;
            let reached;
            const waiting = new Promise(resolve => reached = resolve);
            if (mode === "chat-pending") {
                const chat = require(${JSON.stringify(require.resolve("../src/realtime/chat"))});
                const start = chat.StartChat;
                chat.StartChat = async () => {
                    reached();
                    await new Promise(resolve => release = resolve);
                    return start();
                };
            }
            if (mode === "http-pending" || mode === "ready" || mode === "chat-disabled") {
                const { app } = require(${JSON.stringify(require.resolve("../src/app"))});
                const listen = app.listen.bind(app);
                app.listen = (port, host, callback) => listen(port, host, error => {
                    assert.ifError(error);
                    if (mode === "http-pending") release = () => callback(error);
                    else callback(error);
                    reached();
                });
            }
            const service = require(${JSON.stringify(require.resolve("../src/server"))});
            (async () => {
                if (mode !== "early") await waiting;
                let chatClient;
                let httpClosed;
                if (mode === "ready") {
                    const WebSocket = require("ws");
                    chatClient = new WebSocket("ws://127.0.0.1:${ChatPort}/");
                    await new Promise((resolve, reject) => { chatClient.once("open", resolve); chatClient.once("error", reject); });
                    const socket = net.connect(${Port}, "127.0.0.1");
                    await new Promise((resolve, reject) => { socket.once("connect", resolve); socket.once("error", reject); });
                    httpClosed = new Promise(resolve => socket.once("close", resolve));
                    socket.write("GET / HTTP/1.1\\r\\nHost: localhost\\r\\n");
                }
                const closing = service.shutdown();
                assert.equal(service.shutdown(), closing);
                release?.();
                await closing;
                assert.equal(require(${JSON.stringify(require.resolve("../src/db"))}).GetDb().$client.open, false);
                await httpClosed;
                if (chatClient) assert.equal(chatClient.readyState, 3);
                await service.shutdown();
                for (const port of [${Port}, ${ChatPort}]) {
                    const listener = net.createServer();
                    await new Promise((resolve, reject) => {
                        listener.once("error", reject);
                        listener.listen(port, "127.0.0.1", resolve);
                    });
                    await new Promise(resolve => listener.close(resolve));
                }
                process.send("closed");
                process.disconnect();
            })().catch(error => { console.error(error); process.exit(1); });
        `;
        const Child = spawn(process.execPath, ["-e", Script], {
            cwd: process.cwd(),
            env: { ...process.env, DB_FILENAME: path.join(Dir, "test.db"), PORT: String(Port), BIND_HOST: "127.0.0.1",
                CHAT: Mode === "chat-disabled" ? "0" : "1", CHAT_PORT: String(ChatPort), CHAT_BIND_HOST: "127.0.0.1" },
            stdio: ["ignore", "pipe", "pipe", "ipc"]
        });
        let Output = "";
        let Closed = false;
        Child.stdout!.on("data", Chunk => Output += Chunk);
        Child.stderr!.on("data", Chunk => Output += Chunk);
        Child.on("message", Message => { if(Message === "closed") Closed = true; });
        const Deadline = setTimeout(() => Child.kill(), 15_000);
        try{
            const [Code, Signal] = await once(Child, "exit");
            assert.equal(Signal, null, Output);
            assert.equal(Code, 0, Output);
            assert.equal(Closed, true, Output);
        }
        finally{
            clearTimeout(Deadline);
            fs.rmSync(Dir, {recursive: true, force: true});
        }
    });
}
