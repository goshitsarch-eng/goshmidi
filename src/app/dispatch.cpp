/*
    Gosh MIDI Player — Qt6/Kirigami
    Copyright (C) 2006-2026 Pedro Lopez-Cabanillas and contributors
*/

#include "dispatch.hpp"

#include <QCoreApplication>
#include <QMetaObject>

namespace dmidi {

void postToUiThread(std::function<void()> fn)
{
    if (!fn)
        return;
    auto* app = QCoreApplication::instance();
    if (!app) {
        fn();
        return;
    }
    QMetaObject::invokeMethod(app, std::move(fn), Qt::QueuedConnection);
}

} // namespace dmidi
