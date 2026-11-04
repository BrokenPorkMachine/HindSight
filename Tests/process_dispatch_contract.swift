import Foundation

@main
struct ProcessDispatchContract {
    static func main() throws {
        func run(_ script: String, input: Data = Data(), timeout: TimeInterval = 2,
                 limit: Int = 1024 * 1024) throws -> BoundedProcessRunner.Output {
            try BoundedProcessRunner.run(executable: URL(fileURLWithPath: "/bin/sh"),
                                         arguments: ["-c", script], input: input,
                                         timeout: timeout, limit: limit)
        }
        let payload = Data(repeating: 65, count: 512 * 1024)
        let echo = try run("cat; printf diagnostic >&2", input: payload)
        precondition(echo.status == 0 && echo.stdout == payload)
        precondition(String(decoding: echo.stderr, as: UTF8.self) == "diagnostic")

        let start = Date()
        let blocked = try run("exec sleep 10", input: payload, timeout: 0.15)
        precondition(blocked.timedOut && Date().timeIntervalSince(start) < 2)

        let inheritedStart = Date()
        let inherited = try run("sleep 2 & printf done", timeout: 0.5)
        precondition(inherited.status == 0 && !inherited.timedOut)
        precondition(String(decoding: inherited.stdout, as: UTF8.self) == "done")
        precondition(Date().timeIntervalSince(inheritedStart) < 1.5)

        let flood = try run("head -c 2097152 /dev/zero", limit: 4096)
        precondition(flood.status == 0 && flood.exceeded && flood.stdout.count == 4096)

        let ignored = try run("trap '' TERM; while :; do :; done", timeout: 0.1)
        precondition(ignored.timedOut && ignored.status != nil)

        let earlyExit = try run("exit 7", input: payload)
        precondition(earlyExit.status == 7 && !earlyExit.timedOut)

        let host = ScriptingHost()
        let reply = host.run(source: "print('dispatch-ok')", contextJSON: "{}",
                             capabilities: [], timeoutMS: 2000)
        precondition(reply.contains("dispatch-ok"), reply)
        print("Process dispatch contracts passed: input backpressure, inherited pipes, output bounds, forced termination, early exit, scripting integration.")
    }
}
