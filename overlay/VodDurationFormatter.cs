namespace SOOPLiveWinUI;

public static class VodDurationFormatter
{
    public static string Format(string encoded)
    {
        var fields = encoded.Split(new[] { '|' }, 2);
        if (fields.Length == 0 || !int.TryParse(fields[0], out var part)) return "";
        if (fields.Length != 2 ||
            !long.TryParse(fields[1], System.Globalization.NumberStyles.Integer,
                System.Globalization.CultureInfo.InvariantCulture, out var seconds) || seconds <= 0)
            return $"PART {part} : 시간 정보 없음";

        var totalMinutes = Math.Max(1L, (long)Math.Ceiling(seconds / 60d));
        return $"PART {part} : {totalMinutes / 60:00}시 {totalMinutes % 60:00}분";
    }
}
