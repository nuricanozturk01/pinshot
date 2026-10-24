// pinshot-audiotap: records what the Mac is playing ("system sound") with ScreenCaptureKit.
//
//   pinshot-audiotap <output.pcm>   writes interleaved signed 16-bit little-endian PCM, 48 kHz, stereo
//   pinshot-audiotap --check        prints OK and exits (used by the self-test)
//
// Prints "started" on stdout once capture is running. Stops on SIGTERM, SIGINT, a line starting with "q" on stdin,
// or end of stdin (so it never outlives PinShot).
// Exit codes: 0 ok, 2 Screen Recording permission missing, 3 no display, 4 capture error, 64 usage, 65 macOS too old.
// Needs macOS 13+ (audio capture) and the Screen Recording permission of the app that launched it.

import AVFoundation
import CoreAudio
import CoreMedia
import Foundation
import ScreenCaptureKit

func fail(_ message: String, code: Int32) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(code)
}

enum TapError: Error {
    case noDisplay
}

let arguments = CommandLine.arguments
if arguments.count >= 2 && arguments[1] == "--check" {
    print("OK")
    exit(0)
}
guard arguments.count >= 2 else { fail("usage: pinshot-audiotap <output.pcm> | --check", code: 64) }
guard #available(macOS 13.0, *) else { fail("macOS 13 or newer is required for system sound", code: 65) }

@available(macOS 13.0, *)
final class AudioTap: NSObject, SCStreamOutput, SCStreamDelegate {
    private let handle: FileHandle
    private var stream: SCStream?
    private let queue = DispatchQueue(label: "pinshot.audiotap.samples")

    init(path: String) throws {
        FileManager.default.createFile(atPath: path, contents: nil)
        handle = try FileHandle(forWritingTo: URL(fileURLWithPath: path))
        super.init()
    }

    func start() async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
        guard let display = content.displays.first else { throw TapError.noDisplay }
        let filter = SCContentFilter(display: display, excludingWindows: [])
        let config = SCStreamConfiguration()
        config.capturesAudio = true
        config.sampleRate = 48000
        config.channelCount = 2
        config.width = 2                    // we only want the sound: keep the (unused) video stream tiny and slow
        config.height = 2
        config.minimumFrameInterval = CMTime(value: 1, timescale: 1)
        let newStream = SCStream(filter: filter, configuration: config, delegate: self)
        try newStream.addStreamOutput(self, type: .audio, sampleHandlerQueue: queue)
        try await newStream.startCapture()
        stream = newStream
    }

    func stop() async {
        try? await stream?.stopCapture()
        try? handle.close()
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .audio, sampleBuffer.isValid else { return }
        do {
            try sampleBuffer.withAudioBufferList { list, _ in
                guard let description = sampleBuffer.formatDescription?.audioStreamBasicDescription else { return }
                let frames = CMSampleBufferGetNumSamples(sampleBuffer)
                guard frames > 0 else { return }
                let buffers = UnsafeMutableAudioBufferListPointer(list.unsafeMutablePointer)
                let planar = (description.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0
                let channels = max(1, Int(description.mChannelsPerFrame))
                var pcm = [Int16](repeating: 0, count: frames * 2)

                func toInt16(_ value: Float) -> Int16 {
                    return Int16(max(-1, min(1, value)) * 32767)
                }

                if planar {                 // one buffer per channel (what ScreenCaptureKit delivers)
                    for index in 0..<min(2, buffers.count) {
                        guard let data = buffers[index].mData else { continue }
                        let samples = data.assumingMemoryBound(to: Float.self)
                        let count = min(frames, Int(buffers[index].mDataByteSize) / MemoryLayout<Float>.size)
                        for i in 0..<count { pcm[i * 2 + index] = toInt16(samples[i]) }
                    }
                    if buffers.count == 1 {
                        for i in 0..<frames { pcm[i * 2 + 1] = pcm[i * 2] }
                    }
                } else if buffers.count > 0, let data = buffers[0].mData {      // interleaved
                    let samples = data.assumingMemoryBound(to: Float.self)
                    let count = min(frames, Int(buffers[0].mDataByteSize) / (MemoryLayout<Float>.size * channels))
                    for i in 0..<count {
                        let left = toInt16(samples[i * channels])
                        pcm[i * 2] = left
                        pcm[i * 2 + 1] = channels > 1 ? toInt16(samples[i * channels + 1]) : left
                    }
                }
                pcm.withUnsafeBytes { raw in
                    try? handle.write(contentsOf: Data(raw))
                }
            }
        } catch {
            // one dropped buffer is not worth stopping the recording for
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        fail("capture stopped: \(error.localizedDescription)", code: 4)
    }
}

let tap: AudioTap
do {
    tap = try AudioTap(path: arguments[1])
} catch {
    fail("cannot open the output file: \(error.localizedDescription)", code: 4)
}

func shutdown(_ code: Int32) {
    Task {
        await tap.stop()
        exit(code)
    }
}

Task {
    do {
        try await tap.start()
        print("started")
        fflush(stdout)
    } catch TapError.noDisplay {
        fail("no display found", code: 3)
    } catch {
        let text = "\(error)".lowercased()
        if text.contains("declined") || text.contains("not permitted") || text.contains("tcc")
            || (error as NSError).code == -3801 {
            fail("Screen Recording permission is missing", code: 2)
        }
        fail("could not start capture: \(error.localizedDescription)", code: 4)
    }
}

signal(SIGTERM, SIG_IGN)
signal(SIGINT, SIG_IGN)
let terminate = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
terminate.setEventHandler { shutdown(0) }
terminate.resume()
let interrupt = DispatchSource.makeSignalSource(signal: SIGINT, queue: .main)
interrupt.setEventHandler { shutdown(0) }
interrupt.resume()

DispatchQueue.global().async {
    while let line = readLine() {            // "q" or end of input: PinShot asks us to stop (or PinShot is gone)
        if line.lowercased().hasPrefix("q") { break }
    }
    shutdown(0)
}

RunLoop.main.run()
