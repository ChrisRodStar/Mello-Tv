import Foundation

private final class ReadinessResponse: URLProtocol {
    static var status = 200
    static var body = ""
    static var failure: URLError?
    static var requests = 0

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}

    override func startLoading() {
        Self.requests += 1
        precondition(request.url?.absoluteString == "http://127.0.0.1:8080/api/v1/update-readiness")
        if let failure = Self.failure {
            client?.urlProtocol(self, didFailWithError: failure)
            return
        }
        let response = HTTPURLResponse(url: request.url!, statusCode: Self.status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(Self.body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
}

@main
struct UpdateReadinessProbeTests {
    static func main() async {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [ReadinessResponse.self]
        let session = URLSession(configuration: configuration)
        defer { session.invalidateAndCancel() }
        let probe = UpdateReadinessProbe(port: 8080, session: session)

        let cases: [(String, Int, String, URLError?, Bool)] = [
            ("active playback", 200, #"{"safe_to_restart":false,"active_streams":1}"#, nil, false),
            ("idle server", 200, #"{"safe_to_restart":true,"active_streams":0}"#, nil, true),
            ("old backend", 404, "{}", nil, false),
            ("unavailable activity", 503, #"{"safe_to_restart":true}"#, nil, false),
            ("missing readiness", 200, "{}", nil, false),
            ("malformed response", 200, "not json", nil, false),
            ("connection failure", 200, "", URLError(.cannotConnectToHost), false)
        ]
        for (name, status, body, failure, expected) in cases {
            ReadinessResponse.status = status
            ReadinessResponse.body = body
            ReadinessResponse.failure = failure
            let actual = await probe.canRestartServer(isServerRunning: true)
            precondition(actual == expected, name)
            print("PASS: \(name)")
        }
        let requests = ReadinessResponse.requests
        let stopped = await probe.canRestartServer(isServerRunning: false)
        precondition(stopped && ReadinessResponse.requests == requests, "stopped server must not need an HTTP request")
        print("PASS: stopped server")
    }
}
