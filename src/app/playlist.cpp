/*
    Drumstick MIDI File Player — GTK4/libadwaita rewrite
*/

#include "playlist.hpp"
#include "../midi/events.hpp"

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <random>

namespace dmidi {

bool Playlist::load(const std::string& fileName)
{
    std::ifstream in(fileName);
    if (!in)
        return false;
    m_items.clear();
    std::filesystem::path base = std::filesystem::path(fileName).parent_path();
    std::string line;
    while (std::getline(in, line)) {
        if (!line.empty() && line.back() == '\r')
            line.pop_back();
        if (line.empty())
            continue;
        std::filesystem::path p(line);
        if (p.is_relative())
            p = base / p;
        if (isSupportedMidiFile(p.string()))
            m_items.push_back(std::filesystem::weakly_canonical(p).string());
    }
    m_file = fileName;
    m_current = m_items.empty() ? -1 : 0;
    m_dirty = false;
    return true;
}

bool Playlist::save(const std::string& fileName) const
{
    std::ofstream out(fileName);
    if (!out)
        return false;
    for (auto& item : m_items)
        out << item << '\n';
    const_cast<Playlist*>(this)->m_file = fileName;
    const_cast<Playlist*>(this)->m_dirty = false;
    return true;
}

void Playlist::clear()
{
    if (!m_items.empty()) {
        m_items.clear();
        m_current = -1;
        m_dirty = true;
    }
}

void Playlist::shuffle()
{
    static std::random_device rd;
    static std::mt19937 rng(rd());
    std::shuffle(m_items.begin(), m_items.end(), rng);
    m_dirty = true;
}

void Playlist::setItems(std::vector<std::string> items)
{
    m_items = std::move(items);
    m_current = m_items.empty() ? -1 : 0;
    m_dirty = true;
}

void Playlist::add(const std::string& path)
{
    if (!isSupportedMidiFile(path))
        return;
    m_items.push_back(std::filesystem::weakly_canonical(path).string());
    if (m_current < 0)
        m_current = 0;
    m_dirty = true;
}

void Playlist::removeAt(int index)
{
    if (index < 0 || index >= size())
        return;
    m_items.erase(m_items.begin() + index);
    if (m_current >= size())
        m_current = size() - 1;
    m_dirty = true;
}

void Playlist::moveUp(int index)
{
    if (index <= 0 || index >= size())
        return;
    std::swap(m_items[index - 1], m_items[index]);
    if (m_current == index)
        m_current--;
    else if (m_current == index - 1)
        m_current++;
    m_dirty = true;
}

void Playlist::moveDown(int index)
{
    if (index < 0 || index >= size() - 1)
        return;
    std::swap(m_items[index], m_items[index + 1]);
    if (m_current == index)
        m_current++;
    else if (m_current == index + 1)
        m_current--;
    m_dirty = true;
}

std::string Playlist::current() const
{
    if (m_current < 0 || m_current >= size())
        return {};
    return m_items[static_cast<size_t>(m_current)];
}

void Playlist::setCurrentIndex(int i)
{
    if (i >= 0 && i < size())
        m_current = i;
}

bool Playlist::selectFirst()
{
    if (empty())
        return false;
    m_current = 0;
    return true;
}

bool Playlist::selectNext()
{
    if (m_current + 1 < size()) {
        m_current++;
        return true;
    }
    return false;
}

bool Playlist::selectPrev()
{
    if (m_current > 0) {
        m_current--;
        return true;
    }
    return false;
}

bool Playlist::atLast() const
{
    return m_current == size() - 1;
}

bool Playlist::atFirst() const
{
    return m_current < 1;
}

} // namespace dmidi
