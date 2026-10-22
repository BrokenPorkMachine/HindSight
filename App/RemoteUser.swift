import Foundation

/// The capabilities a project grant can carry.
///
/// The names are the feature identifiers the remote API advertises, so a
/// grant and the advertised capability set never drift apart.
enum RemoteCapability {
    /// Read the open project snapshot.
    static let projectRead = "project_read"
    /// Submit an analysis job for a captured record.
    static let analysisJobs = "analysis_jobs"
    /// Poll a submitted job and read its result.
    static let jobPoll = "job_poll"
    /// Cancel a submitted job.
    static let jobCancel = "job_cancel"
    /// Every capability the v1 analysis API implements.
    static let all: Set<String> = [projectRead, analysisJobs, jobPoll, jobCancel]
}

/// Remote user identity, bound to a certificate or token.
struct RemoteUser: Codable, Sendable, Hashable {
    let id: String                    // stable opaque identifier
    let subject: String               // certificate subject or token identifier
    let created: Date
    var name: String?                 // optional display name
    var email: String?                // optional contact
    var isActive: Bool = true

    init(id: String = UUID().uuidString, subject: String, name: String? = nil, email: String? = nil) {
        self.id = id
        self.subject = subject
        self.created = Date()
        self.name = name
        self.email = email
    }
}

/// A grant giving a user access to a specific project with defined capabilities.
struct ProjectGrant: Codable, Sendable, Hashable {
    let userID: String
    let projectPath: String           // absolute path to the project directory
    let recordID: String?             // specific record, or nil for all records in project
    let capabilities: Set<String>     // see RemoteCapability
    let created: Date
    let grantedBy: String             // admin user ID who created the grant
    var expires: Date?                // optional expiration
    var isActive: Bool = true

    /// Whether the grant's expiry has passed.
    var isExpired: Bool {
        guard let expires else { return false }
        return expires <= Date()
    }

    /// Whether this grant covers the given project and record for the required capability.
    ///
    /// The direction of the record check is deliberately conservative. A grant
    /// that names no record covers every record in its project, but a grant
    /// that names one record never covers a request that addresses the whole
    /// project, because that request would expose the records the grant does
    /// not name.
    func covers(projectPath: String, recordID: String?, capability: String) -> Bool {
        guard isActive, !isExpired,
              self.projectPath == projectPath,
              capabilities.contains(capability) else { return false }
        switch (self.recordID, recordID) {
        case (nil, _): return true
        case (_, nil): return false
        case (let granted?, let requested?): return granted == requested
        }
    }
}

/// An audit record for a remote operation.
struct RemoteAuditEntry: Codable, Sendable {
    let id: String
    let timestamp: Date
    let userID: String?
    let userSubject: String?
    let method: String
    let path: String
    let projectPath: String?
    let recordID: String?
    let requestID: String?
    let statusCode: Int
    let bytesIn: Int
    let bytesOut: Int
    let latencyMs: Int
    let error: String?
    let clientAddress: String?

    init(user: RemoteUser?, request: RemoteRequest, response: RemoteResponse,
         scope: RemoteScope?, clientAddress: String?, latencyMs: Int) {
        self.id = UUID().uuidString
        self.timestamp = Date()
        self.userID = user?.id
        self.userSubject = user?.subject
        self.method = request.method
        self.path = request.path
        self.projectPath = scope?.projectPath
        self.recordID = scope?.recordID
        self.requestID = scope?.requestID
        self.statusCode = response.code
        self.bytesIn = request.body.count
        self.bytesOut = response.body.utf8.count
        self.latencyMs = latencyMs
        self.error = response.code >= 400 ? response.body : nil
        self.clientAddress = clientAddress
    }
}

/// The remote user registry and grant store.
///
/// The gateway reads the registry from its own queue while the app updates it
/// on the main actor, so every access is serialized by one lock. The store
/// keeps users, grants and the audit trail in the private Application Support
/// directory. Bearer tokens are never persisted: they are per-start
/// credentials that map to a user for the lifetime of the gateway.
final class RemoteUserRegistry: @unchecked Sendable {    private struct Store: Codable {
        let schema: Int
        var users: [RemoteUser]
        var grants: [ProjectGrant]
        var audit: [RemoteAuditEntry]
    }

    static let maxStoreBytes = 16 * 1024 * 1024
    private let storageURL: URL?
    private let lock = NSLock()
    private var users: [String: RemoteUser] = [:]           // keyed by user ID
    private var usersBySubject: [String: String] = [:]      // subject -> user ID
    private var tokens: [(token: String, userID: String)] = []  // ephemeral; never persisted
    private var grants: [String: [ProjectGrant]] = [:]      // user ID -> grants
    private var auditLog: [RemoteAuditEntry] = []
    private let maxAuditEntries = 100_000
    private let maxTokens = 64
    private(set) var persistenceError: String?

    init(storageURL: URL? = nil) {
        self.storageURL = storageURL
        if let storageURL {
            do { try load(from: storageURL) }
            catch { persistenceError = error.localizedDescription }
        }
    }

    /// Resolve the user from the in-memory bearer-token binding.
    ///
    /// The token list is short and bounded, and every comparison runs without
    /// a content-dependent early exit, so a wrong token does not reveal how
    /// much of the right token it matched.
    func user(for token: String) -> RemoteUser? {
        lock.lock(); defer { lock.unlock() }
        guard let match = tokens.first(where: { constantTimeEqual($0.token, token) }) else { return nil }
        guard let user = users[match.userID], user.isActive else { return nil }
        return user
    }

    /// Bind an ephemeral gateway bearer token to a user for this gateway
    /// lifetime. Tokens are never persisted as user identities.
    @discardableResult
    func bindToken(_ token: String, to user: RemoteUser) -> RemoteUser {
        lock.lock(); defer { lock.unlock() }
        users[user.id] = user
        tokens.removeAll { $0.token == token }
        if tokens.count >= maxTokens { tokens.removeFirst(tokens.count - maxTokens + 1) }
        tokens.append((token, user.id))
        grants[user.id, default: []] = grants[user.id, default: []]
        return user
    }

    /// Replace an identity's active bearer token without persisting the token.
    @discardableResult
    func replaceToken(_ token: String, for userID: String) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard token.utf8.count >= 32, let user = users[userID], user.isActive else { return false }
        tokens.removeAll { $0.userID == userID || $0.token == token }
        if tokens.count >= maxTokens { tokens.removeFirst(tokens.count - maxTokens + 1) }
        tokens.append((token, userID))
        return true
    }

    /// Forget a bearer token. The user, the grants and the audit trail stay.
    func revokeToken(_ token: String) {
        lock.lock(); defer { lock.unlock() }
        tokens.removeAll { $0.token == token }
    }

    /// Revoke all ephemeral bearer tokens when the hosted gateway stops.
    func revokeAllTokens() {
        lock.lock(); defer { lock.unlock() }
        tokens.removeAll(keepingCapacity: true)
    }

    /// Enable or disable a remote identity. Disabling immediately invalidates
    /// every in-memory bearer token bound to that user; re-enabling does not
    /// resurrect those credentials.
    @discardableResult
    func setUserActive(_ userID: String, active: Bool) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard var user = users[userID] else { return false }
        guard user.isActive != active else { return true }
        let previous = user
        user.isActive = active
        users[userID] = user
        if !active { tokens.removeAll { $0.userID == userID } }
        persist()
        guard persistenceError == nil else {
            // In particular, never enable an identity in memory when the
            // durable authorization state could not be updated. Tokens
            // revoked during a failed disable remain revoked (fail closed).
            users[userID] = previous
            return false
        }
        return true
    }

    /// Create or retrieve a user by subject (certificate CN or token identifier).
    func user(forSubject subject: String, name: String? = nil, email: String? = nil) -> RemoteUser {
        lock.lock(); defer { lock.unlock() }
        if let existingID = usersBySubject[subject], let existing = users[existingID] {
            return existing
        }
        let user = RemoteUser(subject: subject, name: name, email: email)
        users[user.id] = user
        usersBySubject[subject] = user.id
        grants[user.id] = []
        persist()
        return user
    }

    /// Create a host-enrolled identity only if its durable record can be
    /// committed. Enrollment never creates a project grant or persists a token.
    func enroll(subject: String, name: String) -> RemoteUser? {
        lock.lock(); defer { lock.unlock() }
        guard !subject.isEmpty, subject.utf8.count <= 256,
              !name.isEmpty, name.utf8.count <= 128,
              usersBySubject[subject] == nil else { return nil }
        let user = RemoteUser(subject: subject, name: name)
        users[user.id] = user
        usersBySubject[subject] = user.id
        grants[user.id] = []
        persist()
        guard persistenceError == nil else {
            users.removeValue(forKey: user.id)
            usersBySubject.removeValue(forKey: subject)
            grants.removeValue(forKey: user.id)
            return nil
        }
        return user
    }

    /// Grant a user access to a project (or specific record) with the given capabilities.
    func grant(userID: String, projectPath: String, recordID: String?, capabilities: Set<String>,
               grantedBy: String, expires: Date? = nil) -> ProjectGrant? {
        lock.lock(); defer { lock.unlock() }
        if let existing = grants[userID]?.first(where: {
            $0.isActive && $0.projectPath == projectPath && $0.recordID == recordID &&
            $0.capabilities == capabilities && $0.grantedBy == grantedBy && $0.expires == expires
        }) { return existing }
        let previous = grants[userID]
        let grant = ProjectGrant(userID: userID, projectPath: projectPath, recordID: recordID,
                                 capabilities: capabilities, created: Date(),
                                 grantedBy: grantedBy, expires: expires)
        grants[userID, default: []].append(grant)
        persist()
        guard persistenceError == nil else {
            // Authorization state must never become effective only in memory
            // when its durable write fails.
            grants[userID] = previous
            return nil
        }
        return grant
    }

    /// Revoke a grant.
    @discardableResult
    func revokeGrant(userID: String, grantIndex: Int) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard let values = grants[userID], values.indices.contains(grantIndex) else { return false }
        grants[userID]?.remove(at: grantIndex)
        persist()
        return persistenceError == nil
    }

    /// All grants for a user.
    func grants(for userID: String) -> [ProjectGrant] {
        lock.lock(); defer { lock.unlock() }
        return grants[userID] ?? []
    }

    /// All active grants for a user.
    func activeGrants(for userID: String) -> [ProjectGrant] {
        lock.lock(); defer { lock.unlock() }
        return activeGrantsLocked(userID)
    }

    /// Check if a user has a grant covering the project/record/capability.
    func user(_ user: RemoteUser, can capability: String, projectPath: String, recordID: String?) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return activeGrantsLocked(user.id).contains {
            $0.covers(projectPath: projectPath, recordID: recordID, capability: capability)
        }
    }

    /// Record an audit entry.
    func record(_ entry: RemoteAuditEntry) {
        lock.lock(); defer { lock.unlock() }
        auditLog.append(entry)
        if auditLog.count > maxAuditEntries { auditLog.removeFirst(auditLog.count - maxAuditEntries) }
        persist()
    }

    /// Recent audit entries (for admin review).
    func recentAudit(limit: Int = 1000) -> [RemoteAuditEntry] {
        lock.lock(); defer { lock.unlock() }
        return Array(auditLog.suffix(limit))
    }

    /// All users (for admin UI).
    var allUsers: [RemoteUser] {
        lock.lock(); defer { lock.unlock() }
        return Array(users.values).sorted { $0.created > $1.created }
    }

    /// Write the store to its backing file.
    func flush() {
        lock.lock(); defer { lock.unlock() }
        persist()
    }

    /// The active grants of a user, without taking the lock again.
    private func activeGrantsLocked(_ userID: String) -> [ProjectGrant] {
        (grants[userID] ?? []).filter { $0.isActive && !$0.isExpired }
    }

    private func load(from url: URL) throws {
        let fm = FileManager.default
        if !fm.fileExists(atPath: url.path) { return }
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true else {
            throw NSError(domain: "HindSightRemoteUsers", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Remote identity store must be a regular file."])
        }
        let data = try Data(contentsOf: url, options: [.mappedIfSafe])
        guard data.count <= Self.maxStoreBytes else {
            throw NSError(domain: "HindSightRemoteUsers", code: 2,
                          userInfo: [NSLocalizedDescriptionKey: "Remote identity store exceeds its size limit."])
        }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let store = try decoder.decode(Store.self, from: data)
        let userIDs = store.users.map(\.id)
        let subjects = store.users.map(\.subject)
        guard store.schema == 1, store.users.count <= 10_000,
              store.grants.count <= 100_000, store.audit.count <= maxAuditEntries,
              Set(userIDs).count == userIDs.count, Set(subjects).count == subjects.count,
              store.users.allSatisfy({ !$0.id.isEmpty && !$0.subject.isEmpty }),
              store.grants.allSatisfy({ grant in
                  !grant.userID.isEmpty && grant.projectPath.hasPrefix("/") &&
                  store.users.contains(where: { $0.id == grant.userID })
              }) else {
            throw NSError(domain: "HindSightRemoteUsers", code: 3,
                          userInfo: [NSLocalizedDescriptionKey: "Remote identity store failed validation."])
        }
        users = Dictionary(uniqueKeysWithValues: store.users.map { ($0.id, $0) })
        usersBySubject = Dictionary(uniqueKeysWithValues: store.users.map { ($0.subject, $0.id) })
        grants = Dictionary(grouping: store.grants, by: \.userID)
        auditLog = store.audit
    }

    private func persist() {
        guard let storageURL else { return }
        do {
            let directory = storageURL.deletingLastPathComponent()
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true,
                                                     attributes: [.posixPermissions: 0o700])
            let directoryValues = try directory.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
            guard directoryValues.isDirectory == true, directoryValues.isSymbolicLink != true else {
                throw NSError(domain: "HindSightRemoteUsers", code: 6,
                              userInfo: [NSLocalizedDescriptionKey: "Remote identity directory must be a real directory."])
            }
            try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: directory.path)
            if FileManager.default.fileExists(atPath: storageURL.path) {
                let values = try storageURL.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey])
                guard values.isRegularFile == true, values.isSymbolicLink != true else {
                    throw NSError(domain: "HindSightRemoteUsers", code: 4,
                                  userInfo: [NSLocalizedDescriptionKey: "Remote identity store must be a regular file."])
                }
            }
            let store = Store(schema: 1, users: Array(users.values),
                              grants: grants.values.flatMap { $0 }, audit: auditLog)
            let encoder = JSONEncoder()
            encoder.dateEncodingStrategy = .iso8601
            encoder.outputFormatting = [.sortedKeys]
            let data = try encoder.encode(store)
            guard data.count <= Self.maxStoreBytes else {
                throw NSError(domain: "HindSightRemoteUsers", code: 5,
                              userInfo: [NSLocalizedDescriptionKey: "Remote identity store exceeds its size limit."])
            }
            try data.write(to: storageURL, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: storageURL.path)
            persistenceError = nil
        } catch { persistenceError = error.localizedDescription }
    }
}

/// Compare two secrets without a content-dependent early exit.
///
/// The comparison always reads both strings in full and folds the
/// differences, so the time it takes does not depend on how many leading
/// bytes two values share.
func constantTimeEqual(_ a: String, _ b: String) -> Bool {
    let left = Array(a.utf8), right = Array(b.utf8)
    guard left.count == right.count else { return false }
    var difference: UInt8 = 0
    for index in left.indices { difference |= left[index] ^ right[index] }
    return difference == 0
}
