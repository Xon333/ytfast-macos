// Adapted from MacControlCenterUI's VolumeMenuSliderImage.Level.
// Copyright (c) 2026 Steffan Andrews; MIT. See ThirdParty/MacControlCenterUI-LICENSE.txt.
// https://github.com/orchetect/MacControlCenterUI/blob/086922750e4286477431be75032218350441db3c/Sources/MacControlCenterUI/Controls/MenuSlider/MenuSliderImage/VolumeMenuSliderImage.swift
// Preserve its five levels and ordered boundaries, using percentages and SF
// Symbol names directly so the AppKit control needs no SwiftUI host or package.
enum VolumeSymbol {
    static func name(percent: Double) -> String {
        switch percent {
        case ...0: return "speaker.slash.fill"
        case ...16.5: return "speaker.fill"
        case ...33: return "speaker.wave.1.fill"
        case ...66: return "speaker.wave.2.fill"
        default: return "speaker.wave.3.fill"
        }
    }
}
