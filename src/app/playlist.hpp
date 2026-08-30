/*
    Gosh MIDI Player — Qt6/Kirigami
*/

#pragma once

#include <string>
#include <vector>

namespace dmidi {

class Playlist {
public:
    bool load(const std::string& fileName);
    bool save(const std::string& fileName) const;
    void clear();
    void shuffle();

    void setItems(std::vector<std::string> items);
    const std::vector<std::string>& items() const { return m_items; }
    void add(const std::string& path);
    void removeAt(int index);
    void moveUp(int index);
    void moveDown(int index);

    std::string current() const;
    int currentIndex() const { return m_current; }
    void setCurrentIndex(int i);
    bool selectFirst();
    bool selectNext();
    bool selectPrev();
    bool atLast() const;
    bool atFirst() const;
    bool empty() const { return m_items.empty(); }
    int size() const { return static_cast<int>(m_items.size()); }
    bool dirty() const { return m_dirty; }
    void setDirty(bool d) { m_dirty = d; }
    std::string fileName() const { return m_file; }

private:
    std::vector<std::string> m_items;
    std::string m_file;
    int m_current{-1};
    bool m_dirty{};
};

} // namespace dmidi
