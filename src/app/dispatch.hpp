/*
    Gosh MIDI Player — Qt6/Kirigami
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#pragma once

#include <functional>

namespace dmidi {

// Runs fn on the thread owning the Qt event loop, so worker threads (the MIDI
// player loop) can hand results back to the GUI. When no QCoreApplication
// exists — headless tests — fn is invoked directly on the calling thread.
void postToUiThread(std::function<void()> fn);

} // namespace dmidi
