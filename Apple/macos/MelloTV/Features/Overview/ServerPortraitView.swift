import AppKit
import SceneKit
import SwiftUI
import MelloKit

/// Apple's actual Mac mini geometry. Lighting indicates the streaming service's
/// state, not the physical Mac's power state. Render only when the scene changes.
struct ServerPortraitView: View {
    let isOnline: Bool
    let needsAttention: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                Ellipse()
                    .fill(MelloTVTheme.accentGlow)
                    .frame(width: geometry.size.width * 0.68, height: geometry.size.height * 0.28)
                    .blur(radius: 26)
                    .opacity(isOnline ? 0.75 : 0)
                    .offset(y: geometry.size.height * 0.29)

                Ellipse()
                    .fill(.black.opacity(isOnline ? 0.24 : 0.14))
                    .frame(width: geometry.size.width * 0.58, height: 16)
                    .blur(radius: 13)
                    .offset(y: geometry.size.height * 0.32)

                MacMiniScene(isOnline: isOnline, needsAttention: needsAttention)
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
            .animation(reduceMotion ? nil : .easeInOut(duration: 0.35), value: isOnline)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// SceneKit keeps this resource usable on the app's macOS 13 deployment target.
/// AppKit owns the renderer's sizing; no custom layout callbacks are needed.
private struct MacMiniScene: NSViewRepresentable {
    let isOnline: Bool
    let needsAttention: Bool

    func makeNSView(context: Context) -> SCNView {
        let view = SCNView()
        view.backgroundColor = .clear
        view.antialiasingMode = .multisampling4X
        view.rendersContinuously = false
        view.isPlaying = false
        view.allowsCameraControl = false

        guard let url = Bundle.main.url(forResource: "MacMini", withExtension: "usdz") else {
            NSLog("Mello TV: bundled Mac mini model is missing")
            return view
        }
        do {
            let scene = try SCNScene(url: url, options: nil)
            let bounds = scene.rootNode.boundingBox
            let center = SCNVector3((bounds.min.x + bounds.max.x) / 2,
                                   (bounds.min.y + bounds.max.y) / 2,
                                   (bounds.min.z + bounds.max.z) / 2)
            let span = max(bounds.max.x - bounds.min.x,
                           bounds.max.y - bounds.min.y,
                           bounds.max.z - bounds.min.z)
            guard span > 0 else { return view }

            let target = SCNNode()
            target.position = center
            scene.rootNode.addChildNode(target)

            let camera = SCNNode()
            camera.camera = SCNCamera()
            camera.camera?.usesOrthographicProjection = true
            camera.camera?.orthographicScale = Double(span) * 0.72
            camera.camera?.zNear = Double(span) * 0.01
            camera.camera?.zFar = Double(span) * 20
            camera.position = SCNVector3(center.x + span * 1.5,
                                        center.y + span * 1.4,
                                        center.z + span * 2.3)
            let lookAt = SCNLookAtConstraint(target: target)
            lookAt.isGimbalLockEnabled = true
            camera.constraints = [lookAt]
            scene.rootNode.addChildNode(camera)
            view.pointOfView = camera

            let key = light(named: "overview.key", type: .omni, color: .white)
            key.position = SCNVector3(center.x - span * 2, center.y + span * 3, center.z + span * 3)
            scene.rootNode.addChildNode(key)

            let fill = light(named: "overview.fill", type: .ambient, color: .white)
            scene.rootNode.addChildNode(fill)

            let rim = light(named: "overview.rim", type: .omni, color: .systemBlue)
            rim.position = SCNVector3(center.x + span * 2, center.y + span, center.z - span)
            scene.rootNode.addChildNode(rim)

            // A neutral studio environment keeps the aluminum silver in both states.
            scene.lightingEnvironment.contents = NSColor(white: 0.8, alpha: 1)
            scene.background.contents = NSColor.clear
            view.scene = scene
            updateLighting(view)
        } catch {
            NSLog("Mello TV: unable to load the Mac mini model: %@", error.localizedDescription)
        }
        return view
    }

    func updateNSView(_ view: SCNView, context: Context) {
        updateLighting(view)
    }

    private func updateLighting(_ view: SCNView) {
        guard let scene = view.scene else { return }
        scene.rootNode.childNode(withName: "overview.key", recursively: false)?.light?.intensity = isOnline ? 1100 : 430
        scene.rootNode.childNode(withName: "overview.fill", recursively: false)?.light?.intensity = isOnline ? 450 : 220
        let rim = scene.rootNode.childNode(withName: "overview.rim", recursively: false)?.light
        rim?.intensity = isOnline ? 250 : 0
        rim?.color = needsAttention ? NSColor.systemOrange : NSColor(MelloTVTheme.accent)
        scene.lightingEnvironment.intensity = isOnline ? 1.1 : 0.55
        view.needsDisplay = true
    }

    private func light(named name: String, type: SCNLight.LightType, color: NSColor) -> SCNNode {
        let node = SCNNode()
        node.name = name
        node.light = SCNLight()
        node.light?.type = type
        node.light?.color = color
        return node
    }
}
