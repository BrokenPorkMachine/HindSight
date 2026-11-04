import Foundation
import Darwin

/// Synchronous only on a service worker lane. All pipe I/O is nonblocking and
/// shares the execution deadline, including input to an unresponsive interpreter.
enum BoundedProcessRunner {
    struct Output {
        let stdout: Data
        let stderr: Data
        let exceeded: Bool
        let timedOut: Bool
        let status: Int32?
    }

    static func run(executable: URL, arguments: [String], input: Data = Data(),
                    timeout: TimeInterval, limit: Int, mergeOutput: Bool = false) throws -> Output {
        precondition(limit > 0 && timeout > 0)
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        let stdin = Pipe(), stdout = Pipe(), stderr = Pipe()
        process.standardInput = stdin
        process.standardOutput = stdout
        process.standardError = mergeOutput ? stdout : stderr
        let handles = [stdin.fileHandleForReading, stdin.fileHandleForWriting,
                       stdout.fileHandleForReading, stdout.fileHandleForWriting,
                       stderr.fileHandleForReading, stderr.fileHandleForWriting]
        defer { handles.forEach { try? $0.close() } }
        let inputFD = stdin.fileHandleForWriting.fileDescriptor
        let outputFD = stdout.fileHandleForReading.fileDescriptor
        let errorFD = stderr.fileHandleForReading.fileDescriptor
        for fd in [inputFD, outputFD, errorFD] {
            guard fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK) != -1 else {
                throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
            }
        }
        guard fcntl(inputFD, F_SETNOSIGPIPE, 1) != -1 else {
            throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
        }
        let finished = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in finished.signal() }
        try process.run()
        try? stdin.fileHandleForReading.close()
        try? stdout.fileHandleForWriting.close()
        try? stderr.fileHandleForWriting.close()
        let pid = process.processIdentifier
        let grouped = setpgid(pid, pid) == 0
        let deadline = DispatchTime.now() + timeout
        var out = Data(), err = Data(), exceeded = false
        var offset = 0, inputClosed = false

        // Limit work per turn even if a descendant continuously writes output.
        func drain(_ fd: Int32, into data: inout Data) {
            var buffer = [UInt8](repeating: 0, count: 16 * 1024)
            for _ in 0..<64 {
                let count = Darwin.read(fd, &buffer, buffer.count)
                if count < 0 && errno == EINTR { continue }
                guard count > 0 else { break }
                let accepted = min(count, limit - data.count)
                data.append(contentsOf: buffer.prefix(accepted))
                if accepted < count { exceeded = true }
            }
        }
        var timedOut = false
        while true {
            drain(outputFD, into: &out)
            if !mergeOutput { drain(errorFD, into: &err) }
            if !process.isRunning { break }
            if DispatchTime.now() >= deadline { timedOut = true; break }
            if !inputClosed {
                if offset < input.count {
                    let count = input.withUnsafeBytes { bytes in
                        guard let baseAddress = bytes.baseAddress else { return -1 }
                        return Darwin.write(inputFD, baseAddress.advanced(by: offset), min(16 * 1024, input.count - offset))
                    }
                    if count > 0 { offset += count }
                    else if count < 0 && errno != EAGAIN && errno != EINTR { offset = input.count }
                }
                if offset == input.count { try? stdin.fileHandleForWriting.close(); inputClosed = true }
            }
            _ = finished.wait(timeout: min(deadline, .now() + .milliseconds(10)))
        }
        if timedOut && process.isRunning {
            if grouped { _ = kill(-pid, SIGTERM) } else { process.terminate() }
            if finished.wait(timeout: .now() + .milliseconds(250)) == .timedOut {
                if grouped { _ = kill(-pid, SIGKILL) }
                else if process.isRunning { _ = kill(pid, SIGKILL) }
                _ = finished.wait(timeout: .now() + .seconds(1))
            }
        }
        // Never wait for EOF: grandchildren may retain the write descriptors.
        drain(outputFD, into: &out)
        if !mergeOutput { drain(errorFD, into: &err) }
        return Output(stdout: out, stderr: err, exceeded: exceeded, timedOut: timedOut,
                      status: process.isRunning ? nil : process.terminationStatus)
    }
}
