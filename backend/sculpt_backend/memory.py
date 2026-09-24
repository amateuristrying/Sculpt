from .config import SPEC


def check_memory(quality, available_gb):
    profile = next((item for item in SPEC['qualities'] if item['id'] == quality), None)
    if profile is None:
        raise ValueError('Unknown geometry quality.')
    # Estimated peak use includes memory already held by the Python/Torch process.
    # Gate on remaining headroom, not the entire estimated peak a second time.
    needed = profile['minimumAvailableGb']
    if available_gb < needed:
        advice = 'Close other apps, then retry.' if quality == 'draft' else 'Close other apps or choose Draft, then retry.'
        raise ValueError(f'Only {available_gb:.2f} GB of memory is currently available; {quality.title()} needs at least {needed:.2f} GB of headroom. {advice}')
    return profile
