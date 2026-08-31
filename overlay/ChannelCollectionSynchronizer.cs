namespace SOOPLiveWinUI;

internal readonly record struct ChannelSyncResult(int Added, int Removed, int Moved)
{
    internal int TotalChanges => Added + Removed + Moved;
}

internal static class ChannelCollectionSynchronizer
{
    internal static ChannelSyncResult Synchronize<T>(IList<T> current, IReadOnlyList<T> desired)
        where T : class
    {
        ArgumentNullException.ThrowIfNull(current);
        ArgumentNullException.ThrowIfNull(desired);

        var desiredItems = new HashSet<T>(desired, ReferenceEqualityComparer.Instance);
        var removed = 0;
        var added = 0;
        var moved = 0;

        for (var index = current.Count - 1; index >= 0; index--)
        {
            if (desiredItems.Contains(current[index])) continue;
            current.RemoveAt(index);
            removed++;
        }

        for (var targetIndex = 0; targetIndex < desired.Count; targetIndex++)
        {
            var target = desired[targetIndex];
            if (targetIndex < current.Count && ReferenceEquals(current[targetIndex], target))
                continue;

            var existingIndex = IndexOfReference(current, target, targetIndex + 1);
            if (existingIndex >= 0)
            {
                if (current is System.Collections.ObjectModel.ObservableCollection<T> observable)
                    observable.Move(existingIndex, targetIndex);
                else
                {
                    current.RemoveAt(existingIndex);
                    current.Insert(targetIndex, target);
                }
                moved++;
            }
            else
            {
                current.Insert(targetIndex, target);
                added++;
            }
        }

        while (current.Count > desired.Count)
        {
            current.RemoveAt(current.Count - 1);
            removed++;
        }

        return new ChannelSyncResult(added, removed, moved);
    }

    static int IndexOfReference<T>(IList<T> items, T target, int startIndex)
        where T : class
    {
        for (var index = Math.Max(0, startIndex); index < items.Count; index++)
        {
            if (ReferenceEquals(items[index], target)) return index;
        }
        return -1;
    }
}
