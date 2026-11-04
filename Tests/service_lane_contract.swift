import Foundation

// Deterministic admission fixture; the production scheduler is compiled unchanged.
enum ServiceReply { static func failure(_ text: String) -> String { "failure: " + text } }
final class SharedResourceLedger {
    static let shared = SharedResourceLedger()
    func reserve(_ bytes: UInt64) -> UUID? { UUID() }
    func release(_ token: UUID) {}
}

@main
struct ServiceLaneContract {
    static func main() {
        let lane = BoundedServiceLane(name: "test.busy", qos: .utility, maxConcurrent: 1, maxPending: 1)
        let other = BoundedServiceLane(name: "test.other", qos: .utility, maxConcurrent: 1)
        let started = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
        let completed = DispatchSemaphore(value: 0), cancelled = DispatchSemaphore(value: 0)
        precondition(lane.submit(reply: { reply in
            precondition(reply.contains("superseded")); completed.signal()
        }, work: {
            started.signal(); precondition(release.wait(timeout: .now() + 3) == .success)
            return "finished"
        }))
        precondition(started.wait(timeout: .now() + 2) == .success)
        precondition(lane.submit(reply: { reply in
            precondition(reply.contains("cancelled")); cancelled.signal()
        }, work: { fatalError("Cancelled pending operation executed") }))
        precondition(!lane.submit(reply: { precondition($0.contains("saturated")) }, work: { "unexpected" }))
        let independent = DispatchSemaphore(value: 0)
        other.submit(reply: { _ in independent.signal() }, work: { "responsive" })
        precondition(independent.wait(timeout: .now() + 1) == .success)
        lane.cancelQueued()
        precondition(cancelled.wait(timeout: .now() + 1) == .success)
        release.signal()
        precondition(completed.wait(timeout: .now() + 2) == .success)
        let resumed = DispatchSemaphore(value: 0)
        lane.submit(reply: { precondition($0 == "new session"); resumed.signal() }, work: { "new session" })
        precondition(resumed.wait(timeout: .now() + 2) == .success)
        print("Service lane contracts passed: concurrency, saturation, cancellation, independent progress, reuse.")
    }
}
