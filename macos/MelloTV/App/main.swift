import Cocoa

let app = NSApplication.shared
let delegate = MainActor.assumeIsolated { MelloTVAppDelegate() }
app.delegate = delegate
app.setActivationPolicy(.regular)

let terminationSignal = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
terminationSignal.setEventHandler {
    Task { @MainActor in
        delegate.stopServer(waitTimeout: 2)
        exit(0)
    }
}
terminationSignal.resume()
signal(SIGTERM, SIG_IGN)

let interruptSignal = DispatchSource.makeSignalSource(signal: SIGINT, queue: .main)
interruptSignal.setEventHandler {
    Task { @MainActor in
        delegate.stopServer(waitTimeout: 2)
        exit(0)
    }
}
interruptSignal.resume()
signal(SIGINT, SIG_IGN)

app.run()
