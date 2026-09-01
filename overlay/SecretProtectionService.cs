using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;

namespace SOOPLiveWinUI;

internal static class SecretProtectionService
{
    internal const string Prefix = "dpapi:v1:";
    static readonly byte[] Entropy = Encoding.UTF8.GetBytes("SOOPLiveDownloader:v1");
    const int CryptProtectUiForbidden = 0x1;

    [StructLayout(LayoutKind.Sequential)]
    struct DataBlob
    {
        public int Size;
        public IntPtr Data;
    }

    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CryptProtectData(
        ref DataBlob input, string? description, ref DataBlob entropy,
        IntPtr reserved, IntPtr prompt, int flags, out DataBlob output);

    [DllImport("crypt32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    static extern bool CryptUnprotectData(
        ref DataBlob input, IntPtr description, ref DataBlob entropy,
        IntPtr reserved, IntPtr prompt, int flags, out DataBlob output);

    [DllImport("kernel32.dll")]
    static extern IntPtr LocalFree(IntPtr memory);

    internal static bool IsProtected(string? value) =>
        value?.StartsWith(Prefix, StringComparison.OrdinalIgnoreCase) == true;

    internal static string Protect(string? plainText)
    {
        if (string.IsNullOrEmpty(plainText)) return "";
        if (!OperatingSystem.IsWindows())
            throw new PlatformNotSupportedException("DPAPI 자격증명 보호는 Windows에서만 사용할 수 있습니다.");

        var plain = Encoding.UTF8.GetBytes(plainText);
        try
        {
            var encrypted = Transform(plain, protect: true);
            return Prefix + Convert.ToBase64String(encrypted);
        }
        finally
        {
            CryptographicOperations.ZeroMemory(plain);
        }
    }

    internal static string Unprotect(string? storedValue)
    {
        if (string.IsNullOrEmpty(storedValue)) return "";
        if (!IsProtected(storedValue)) return storedValue; // legacy plaintext migration
        if (!OperatingSystem.IsWindows())
            throw new PlatformNotSupportedException("DPAPI 자격증명은 암호화한 Windows 사용자만 해제할 수 있습니다.");

        byte[] encrypted;
        try { encrypted = Convert.FromBase64String(storedValue[Prefix.Length..]); }
        catch (FormatException ex) { throw new InvalidDataException("손상된 DPAPI 자격증명 형식입니다.", ex); }

        var plain = Transform(encrypted, protect: false);
        try { return Encoding.UTF8.GetString(plain); }
        finally { CryptographicOperations.ZeroMemory(plain); }
    }

    internal static string ProtectIniSecretsForCurrentUser(string iniText)
    {
        return Regex.Replace(
            iniText,
            @"^(?<key>SOOP_PASSWORD|CLOUDFLARE_API_KEY)\s*=\s*(?<value>[^\r\n]*)(?<cr>\r?)$",
            match =>
            {
                var value = match.Groups["value"].Value.Trim();
                var ending = match.Groups["cr"].Value;
                if (value.Length == 0) return match.Groups["key"].Value + "=" + ending;
                return match.Groups["key"].Value + "=" + Protect(Unprotect(value)) + ending;
            },
            RegexOptions.Multiline | RegexOptions.IgnoreCase);
    }

    static byte[] Transform(byte[] input, bool protect)
    {
        var inputBlob = Allocate(input);
        var entropyBlob = Allocate(Entropy);
        var outputBlob = new DataBlob();
        try
        {
            var success = protect
                ? CryptProtectData(ref inputBlob, "SOOP LIVE Downloader", ref entropyBlob,
                    IntPtr.Zero, IntPtr.Zero, CryptProtectUiForbidden, out outputBlob)
                : CryptUnprotectData(ref inputBlob, IntPtr.Zero, ref entropyBlob,
                    IntPtr.Zero, IntPtr.Zero, CryptProtectUiForbidden, out outputBlob);
            if (!success) throw new Win32Exception(Marshal.GetLastWin32Error(), "Windows DPAPI 처리에 실패했습니다.");
            var result = new byte[outputBlob.Size];
            Marshal.Copy(outputBlob.Data, result, 0, result.Length);
            return result;
        }
        finally
        {
            Free(ref inputBlob, clear: true);
            Free(ref entropyBlob, clear: false);
            if (outputBlob.Data != IntPtr.Zero)
            {
                for (var index = 0; index < outputBlob.Size; index++) Marshal.WriteByte(outputBlob.Data, index, 0);
                LocalFree(outputBlob.Data);
            }
        }
    }

    static DataBlob Allocate(byte[] bytes)
    {
        var blob = new DataBlob { Size = bytes.Length, Data = Marshal.AllocHGlobal(bytes.Length) };
        Marshal.Copy(bytes, 0, blob.Data, bytes.Length);
        return blob;
    }

    static void Free(ref DataBlob blob, bool clear)
    {
        if (blob.Data == IntPtr.Zero) return;
        if (clear)
        {
            for (var index = 0; index < blob.Size; index++) Marshal.WriteByte(blob.Data, index, 0);
        }
        Marshal.FreeHGlobal(blob.Data);
        blob = default;
    }
}
