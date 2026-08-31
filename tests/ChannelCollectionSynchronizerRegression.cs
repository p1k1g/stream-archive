using System.Collections.ObjectModel;
using SOOPLiveWinUI;

static class ChannelCollectionSynchronizerRegression
{
    internal static void Run()
    {
        var all = Enumerable.Range(0, 10_000).Select(index => new Item(index)).ToArray();
        var visible = new ObservableCollection<Item>(all);
        var changeEvents = 0;
        visible.CollectionChanged += (_, _) => changeEvents++;
        var filtered = all.Where(item => item.Id % 2 == 0).ToArray();

        var filterResult = ChannelCollectionSynchronizer.Synchronize(visible, filtered);
        Check(visible.SequenceEqual(filtered), "large filter must retain reference order");
        Check(filterResult.Removed == 5_000 && filterResult.Added == 0 && filterResult.Moved == 0,
            "large filter must use removals only");
        Check(changeEvents == filterResult.TotalChanges,
            "reported mutations must match observable collection events");

        changeEvents = 0;
        var repeatedResult = ChannelCollectionSynchronizer.Synchronize(visible, filtered);
        Check(repeatedResult.TotalChanges == 0, "unchanged filter must emit no collection changes");
        Check(changeEvents == 0, "unchanged filter must emit no observable collection events");

        var reordered = filtered.Reverse().ToArray();
        changeEvents = 0;
        var reorderResult = ChannelCollectionSynchronizer.Synchronize(visible, reordered);
        Check(visible.SequenceEqual(reordered), "large reorder must match requested order");
        Check(reorderResult.Added == 0 && reorderResult.Removed == 0,
            "reorder must preserve existing item instances");
        Check(changeEvents == reorderResult.Moved,
            "reorder must emit move events only");

        Console.WriteLine("Channel collection synchronizer regression tests passed.");
    }

    static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    sealed record Item(int Id);
}
