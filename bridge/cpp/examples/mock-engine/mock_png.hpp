#pragma once

// @brief モック撮影用の決定的な RGBA8 PNG 符号化と画素要約値。

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace norves::mock::png
{
    inline void AppendBigEndian(std::vector<std::uint8_t>& output, std::uint32_t value)
    {
        for (int shift = 24; shift >= 0; shift -= 8)
        {
            output.push_back(static_cast<std::uint8_t>(value >> shift));
        }
    }

    inline std::uint32_t Crc32(std::span<const std::uint8_t> bytes)
    {
        std::uint32_t crc = 0xffffffffU;
        for (const std::uint8_t byte : bytes)
        {
            crc ^= byte;
            for (int bit = 0; bit < 8; ++bit)
            {
                crc = (crc >> 1) ^ ((crc & 1U) != 0 ? 0xedb88320U : 0U);
            }
        }
        return crc ^ 0xffffffffU;
    }

    inline void AppendChunk(std::vector<std::uint8_t>& output, std::string_view type,
                            std::span<const std::uint8_t> data)
    {
        AppendBigEndian(output, static_cast<std::uint32_t>(data.size()));
        const std::size_t crcBegin = output.size();
        output.insert(output.end(), type.begin(), type.end());
        output.insert(output.end(), data.begin(), data.end());
        AppendBigEndian(output, Crc32(std::span<const std::uint8_t>(output).subspan(crcBegin)));
    }

    // @brief 上から下の、詰め物なしの RGBA8 画素を PNG にする。寸法と画素数は呼び出し側で保証する。
    inline std::vector<std::uint8_t> EncodeRgba8(std::uint32_t width, std::uint32_t height,
                                                 std::span<const std::uint8_t> pixels)
    {
        const std::size_t rowBytes = static_cast<std::size_t>(width) * 4;
        std::vector<std::uint8_t> filtered;
        filtered.reserve((rowBytes + 1) * height);
        for (std::uint32_t y = 0; y < height; ++y)
        {
            filtered.push_back(0);
            const auto row = pixels.subspan(static_cast<std::size_t>(y) * rowBytes, rowBytes);
            filtered.insert(filtered.end(), row.begin(), row.end());
        }

        std::vector<std::uint8_t> zlib{0x78, 0x01};
        for (std::size_t offset = 0; offset < filtered.size();)
        {
            const std::size_t length = (std::min)(filtered.size() - offset, std::size_t{65535});
            const auto block = std::span<const std::uint8_t>(filtered).subspan(offset, length);
            zlib.push_back(offset + length == filtered.size() ? 0x01 : 0x00);
            const auto len = static_cast<std::uint16_t>(length);
            const auto complement = static_cast<std::uint16_t>(~len);
            zlib.push_back(static_cast<std::uint8_t>(len));
            zlib.push_back(static_cast<std::uint8_t>(len >> 8));
            zlib.push_back(static_cast<std::uint8_t>(complement));
            zlib.push_back(static_cast<std::uint8_t>(complement >> 8));
            zlib.insert(zlib.end(), block.begin(), block.end());
            offset += length;
        }

        std::uint32_t a = 1;
        std::uint32_t b = 0;
        for (const std::uint8_t byte : filtered)
        {
            a = (a + byte) % 65521U;
            b = (b + a) % 65521U;
        }
        AppendBigEndian(zlib, (b << 16) | a);

        std::vector<std::uint8_t> output{137, 80, 78, 71, 13, 10, 26, 10};
        std::vector<std::uint8_t> ihdr;
        AppendBigEndian(ihdr, width);
        AppendBigEndian(ihdr, height);
        ihdr.insert(ihdr.end(), {8, 6, 0, 0, 0});
        AppendChunk(output, "IHDR", ihdr);
        AppendChunk(output, "IDAT", zlib);
        AppendChunk(output, "IEND", {});
        return output;
    }

    // @brief filter バイトを含まない RGBA8 の FNV-1a 64 を小文字 16 桁の 16 進で返す。
    inline std::string PixelDigest(std::span<const std::uint8_t> pixels)
    {
        std::uint64_t digest = 0xcbf29ce484222325ULL;
        for (const std::uint8_t byte : pixels)
        {
            digest ^= byte;
            digest *= 0x100000001b3ULL;
        }
        constexpr char HexDigits[] = "0123456789abcdef";
        std::string result(16, '0');
        for (std::size_t i = 0; i < result.size(); ++i)
        {
            result[result.size() - 1 - i] = HexDigits[digest & 0xfU];
            digest >>= 4;
        }
        return result;
    }
}  // namespace norves::mock::png
