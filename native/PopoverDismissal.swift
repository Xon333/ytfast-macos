import AppKit

/// One dismissal owner. A transient popover can auto-close on status-item
/// mouse-down, then its mouse-up action opens it again (native report D1).
/// Keep AppKit's popover/placement, but exclude its anchor from outside-click
/// dismissal. Monitors exist only while shown; no timers or polling are used.
final class PopoverDismissal {
    private weak var popover: NSPopover?
    private weak var button: NSStatusBarButton?
    private var localMonitor: Any?
    private var globalMonitor: Any?
    private var workspaceObservers: [NSObjectProtocol] = []
    var activeObserverCount: Int {
        (localMonitor == nil ? 0 : 1) + (globalMonitor == nil ? 0 : 1) + workspaceObservers.count
    }

    init(popover: NSPopover, button: NSStatusBarButton) { self.popover = popover; self.button = button }
    deinit { stop() }

    func start() {
        guard activeObserverCount == 0 else { return }
        let events: NSEvent.EventTypeMask = [.leftMouseDown, .rightMouseDown, .otherMouseDown]
        localMonitor = NSEvent.addLocalMonitorForEvents(matching: events) { [weak self] event in
            let point = event.window?.convertPoint(toScreen: event.locationInWindow) ?? NSEvent.mouseLocation
            self?.mouseDown(at: point, in: event.window)
            return event // Never swallow the click intended for another control/window.
        }
        globalMonitor = NSEvent.addGlobalMonitorForEvents(matching: events) { [weak self] _ in
            self?.mouseDown(at: NSEvent.mouseLocation, in: nil)
        }
        let center = NSWorkspace.shared.notificationCenter
        workspaceObservers.append(center.addObserver(forName: NSWorkspace.didActivateApplicationNotification, object: nil, queue: .main) { [weak self] notification in
            guard let app = notification.userInfo?[NSWorkspace.applicationUserInfoKey] as? NSRunningApplication else { return }
            self?.applicationActivated(processIdentifier: app.processIdentifier)
        })
        workspaceObservers.append(center.addObserver(forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main) { [weak self] _ in
            self?.popover?.performClose(nil)
        })
    }

    func stop() {
        if let localMonitor { NSEvent.removeMonitor(localMonitor) }
        if let globalMonitor { NSEvent.removeMonitor(globalMonitor) }
        localMonitor = nil; globalMonitor = nil
        let center = NSWorkspace.shared.notificationCenter
        for observer in workspaceObservers { center.removeObserver(observer) }
        workspaceObservers.removeAll()
    }

    func mouseDown(at screenPoint: NSPoint, in eventWindow: NSWindow?) {
        guard let popover, popover.isShown else { return }
        if let button, let window = button.window,
           window.convertToScreen(button.convert(button.bounds, to: nil)).contains(screenPoint) {
            return // The status item's existing mouse-up/AX action alone toggles it.
        }
        var window = eventWindow
        while let current = window {
            if current === popover.contentViewController?.view.window { return }
            window = current.parent // Keep attached native sheets/child controls usable.
        }
        popover.performClose(nil)
    }

    func applicationActivated(processIdentifier: pid_t) {
        if processIdentifier != ProcessInfo.processInfo.processIdentifier { popover?.performClose(nil) }
    }
}
