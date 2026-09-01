namespace SOOPLiveWinUI;

public static class VodSelectionParser
{
    public static IReadOnlyList<int> Parse(string? text, int partCount)
    {
        if (partCount < 1) throw new ArgumentOutOfRangeException(nameof(partCount));
        if (string.IsNullOrWhiteSpace(text) || text.Trim().Equals("all", StringComparison.OrdinalIgnoreCase))
            return Enumerable.Range(1, partCount).ToArray();

        var selected = new SortedSet<int>();
        foreach (var raw in text.Split(','))
        {
            var token = raw.Trim();
            if (token.Length == 0) throw new FormatException("빈 PART 선택 항목이 있습니다.");
            var dash = token.IndexOf('-');
            if (dash >= 0)
            {
                if (dash != token.LastIndexOf('-') ||
                    !int.TryParse(token[..dash].Trim(), out var first) ||
                    !int.TryParse(token[(dash + 1)..].Trim(), out var last) || first > last)
                    throw new FormatException($"잘못된 PART 범위입니다: {token}");
                Validate(first, partCount);
                Validate(last, partCount);
                for (var part = first; part <= last; part++) selected.Add(part);
            }
            else
            {
                if (!int.TryParse(token, out var part)) throw new FormatException($"잘못된 PART 번호입니다: {token}");
                Validate(part, partCount);
                selected.Add(part);
            }
        }
        return selected.ToArray();
    }

    static void Validate(int part, int partCount)
    {
        if (part < 1 || part > partCount)
            throw new ArgumentOutOfRangeException(nameof(part), $"PART는 1~{partCount} 범위여야 합니다: {part}");
    }
}
